use std::fs::{self as std_fs, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;
use std::time::{Duration, SystemTime};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use crate::error::BleatError;

const OLD_TEMP_AGE: Duration = Duration::from_secs(60 * 60);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct SessionLock {
    lock_dir: PathBuf,
}

#[derive(Clone, Copy)]
struct LockPolicy {
    retry_timeout: Duration,
    stale_after: Duration,
    initial_backoff: Duration,
    maximum_backoff: Duration,
}

const DEFAULT_LOCK_POLICY: LockPolicy = LockPolicy {
    retry_timeout: Duration::from_secs(5),
    stale_after: Duration::from_secs(30),
    initial_backoff: Duration::from_millis(10),
    maximum_backoff: Duration::from_millis(100),
};

impl SessionLock {
    pub fn acquire(session_dir: &Path) -> Result<Self, BleatError> {
        Self::acquire_with_policy(session_dir, DEFAULT_LOCK_POLICY)
    }

    fn acquire_with_policy(session_dir: &Path, policy: LockPolicy) -> Result<Self, BleatError> {
        let lock_dir = session_dir.join(".lock");
        let deadline = Instant::now() + policy.retry_timeout;
        let mut backoff = policy.initial_backoff;

        loop {
            match std_fs::create_dir(&lock_dir) {
                Ok(()) => return Self::initialize(lock_dir),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    if stale_lock(&lock_dir, policy.stale_after)? {
                        match std_fs::remove_dir_all(&lock_dir) {
                            Ok(()) => continue,
                            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
                            Err(source) => {
                                return Err(io_error("remove stale lock", &lock_dir, source));
                            }
                        }
                    }
                    if Instant::now() >= deadline {
                        return Err(BleatError::Runtime(format!(
                            "timed out acquiring session lock `{}`",
                            lock_dir.display()
                        )));
                    }
                    thread::sleep(backoff);
                    backoff = std::cmp::min(backoff * 2, policy.maximum_backoff);
                }
                Err(source) => return Err(io_error("create session lock", &lock_dir, source)),
            }
        }
    }

    fn initialize(lock_dir: PathBuf) -> Result<Self, BleatError> {
        let pid_path = lock_dir.join("pid");
        let result = File::create(&pid_path).and_then(|mut file| {
            write!(file, "{}", std::process::id())?;
            file.sync_all()
        });
        if let Err(source) = result {
            let _ = std_fs::remove_dir_all(&lock_dir);
            return Err(io_error("record session lock owner", &pid_path, source));
        }
        Ok(Self { lock_dir })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = std_fs::remove_dir_all(&self.lock_dir);
    }
}

fn stale_lock(lock_dir: &Path, stale_after: Duration) -> Result<bool, BleatError> {
    match std_fs::read_to_string(lock_dir.join("pid")) {
        Ok(contents) => match contents.trim().parse::<i32>() {
            Ok(pid) if pid > 0 => process_is_dead(pid),
            _ => lock_directory_is_old(lock_dir, stale_after),
        },
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            lock_directory_is_old(lock_dir, stale_after)
        }
        Err(source) => Err(io_error("read session lock owner", lock_dir, source)),
    }
}

fn process_is_dead(pid: i32) -> Result<bool, BleatError> {
    match kill(Pid::from_raw(pid), None) {
        Ok(()) | Err(Errno::EPERM) => Ok(false),
        Err(Errno::ESRCH) => Ok(true),
        Err(source) => Err(BleatError::Runtime(format!(
            "failed to inspect lock owner pid {pid}: {source}"
        ))),
    }
}

fn lock_directory_is_old(lock_dir: &Path, stale_after: Duration) -> Result<bool, BleatError> {
    let modified = std_fs::metadata(lock_dir)
        .and_then(|metadata| metadata.modified())
        .map_err(|source| io_error("read session lock metadata", lock_dir, source))?;
    Ok(SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age > stale_after))
}

pub fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), BleatError> {
    let parent = path.parent().ok_or_else(|| {
        BleatError::Runtime(format!("file path has no parent: `{}`", path.display()))
    })?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".tmp-{}-{sequence}", std::process::id()));

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| io_error("create temporary file", &temporary, source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("write temporary file", &temporary, source))?;
    file.sync_all()
        .map_err(|source| io_error("sync temporary file", &temporary, source))?;
    drop(file);

    std_fs::rename(&temporary, path)
        .map_err(|source| io_error("rename temporary file", path, source))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("sync parent directory", parent, source))?;
    Ok(())
}

pub fn cleanup_old_message_temps(
    messages_dir: &Path,
    now: SystemTime,
) -> Result<usize, BleatError> {
    let entries = std_fs::read_dir(messages_dir)
        .map_err(|source| io_error("read messages directory", messages_dir, source))?;
    let mut removed = 0;

    for entry in entries {
        let entry = entry
            .map_err(|source| io_error("read messages directory entry", messages_dir, source))?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(".tmp-") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|source| io_error("read temporary file metadata", &entry.path(), source))?;
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > OLD_TEMP_AGE)
        {
            std_fs::remove_file(entry.path())
                .map_err(|source| io_error("remove old temporary file", &entry.path(), source))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn io_error(action: &str, path: &Path, source: io::Error) -> BleatError {
    BleatError::Runtime(format!("failed to {action} `{}`: {source}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs::{self as std_fs, File, FileTimes};
    use std::time::{Duration, SystemTime};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn atomic_replace_creates_a_new_file() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("session.json");

        atomic_replace(&path, b"first").expect("file should be created");

        assert_eq!(
            std_fs::read(&path).expect("file should be readable"),
            b"first"
        );
    }

    #[test]
    fn atomic_replace_replaces_an_existing_file() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("session.json");
        std_fs::write(&path, b"first").expect("existing file should be written");

        atomic_replace(&path, b"second").expect("file should be replaced");

        assert_eq!(
            std_fs::read(&path).expect("file should be readable"),
            b"second"
        );
    }

    #[test]
    fn atomic_replace_leaves_no_temporary_file() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("session.json");

        atomic_replace(&path, b"content").expect("file should be created");

        assert_eq!(
            std_fs::read_dir(sandbox.path())
                .expect("directory should be readable")
                .count(),
            1
        );
    }

    #[test]
    fn cleanup_removes_only_temp_files_older_than_one_hour() {
        let sandbox = tempdir().expect("sandbox should be created");
        let stale = sandbox.path().join(".tmp-1-1");
        let fresh = sandbox.path().join(".tmp-2-1");
        let message = sandbox.path().join("0001.md");
        let now = SystemTime::now();

        let stale_file = File::create(&stale).expect("stale temp should be created");
        stale_file
            .set_times(FileTimes::new().set_modified(now - Duration::from_secs(3_601)))
            .expect("stale mtime should be set");
        File::create(&fresh).expect("fresh temp should be created");
        File::create(&message).expect("message should be created");

        let removed = cleanup_old_message_temps(sandbox.path(), now)
            .expect("old temp cleanup should succeed");

        assert_eq!(removed, 1);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(message.exists());
    }

    #[test]
    fn session_lock_is_released_when_its_guard_is_dropped() {
        let sandbox = tempdir().expect("sandbox should be created");
        let policy = test_lock_policy(Duration::from_secs(30));

        let guard = SessionLock::acquire_with_policy(sandbox.path(), policy)
            .expect("lock should be acquired");
        assert!(sandbox.path().join(".lock").is_dir());

        drop(guard);
        assert!(!sandbox.path().join(".lock").exists());
        SessionLock::acquire_with_policy(sandbox.path(), policy)
            .expect("released lock should be acquired again");
    }

    #[test]
    fn session_lock_does_not_steal_a_lock_from_a_live_process() {
        let sandbox = tempdir().expect("sandbox should be created");
        let policy = test_lock_policy(Duration::ZERO);
        let _guard = SessionLock::acquire_with_policy(sandbox.path(), policy)
            .expect("first lock should be acquired");

        let error = SessionLock::acquire_with_policy(sandbox.path(), policy)
            .expect_err("live lock should not be stolen");

        assert!(
            error
                .to_string()
                .contains("timed out acquiring session lock")
        );
    }

    #[test]
    fn session_lock_reclaims_a_lock_owned_by_a_dead_process() {
        let sandbox = tempdir().expect("sandbox should be created");
        let lock_dir = sandbox.path().join(".lock");
        std_fs::create_dir(&lock_dir).expect("stale lock should be created");
        std_fs::write(lock_dir.join("pid"), i32::MAX.to_string())
            .expect("dead pid should be recorded");

        let _guard = SessionLock::acquire_with_policy(
            sandbox.path(),
            test_lock_policy(Duration::from_secs(30)),
        )
        .expect("dead lock should be reclaimed");
    }

    #[test]
    fn malformed_pid_in_a_fresh_lock_is_not_reclaimed() {
        let sandbox = tempdir().expect("sandbox should be created");
        let lock_dir = sandbox.path().join(".lock");
        std_fs::create_dir(&lock_dir).expect("lock should be created");
        std_fs::write(lock_dir.join("pid"), "not-a-pid").expect("broken pid should be recorded");

        let error = SessionLock::acquire_with_policy(
            sandbox.path(),
            test_lock_policy(Duration::from_secs(30)),
        )
        .expect_err("fresh malformed lock should not be reclaimed");

        assert!(
            error
                .to_string()
                .contains("timed out acquiring session lock")
        );
    }

    #[test]
    fn malformed_pid_in_an_old_lock_is_reclaimed() {
        let sandbox = tempdir().expect("sandbox should be created");
        let lock_dir = sandbox.path().join(".lock");
        std_fs::create_dir(&lock_dir).expect("lock should be created");
        std_fs::write(lock_dir.join("pid"), "not-a-pid").expect("broken pid should be recorded");

        SessionLock::acquire_with_policy(sandbox.path(), test_lock_policy(Duration::ZERO))
            .expect("old malformed lock should be reclaimed");
    }

    fn test_lock_policy(stale_after: Duration) -> LockPolicy {
        LockPolicy {
            retry_timeout: Duration::from_millis(10),
            stale_after,
            initial_backoff: Duration::from_millis(1),
            maximum_backoff: Duration::from_millis(2),
        }
    }
}
