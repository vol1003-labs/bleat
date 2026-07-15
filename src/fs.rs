use std::fs::{self as std_fs, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use crate::error::BleatError;

const OLD_TEMP_AGE: Duration = Duration::from_secs(60 * 60);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    fn atomic_replace_creates_and_replaces_without_leaving_a_temp_file() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("session.json");

        atomic_replace(&path, b"first").expect("file should be created");
        assert_eq!(
            std_fs::read(&path).expect("file should be readable"),
            b"first"
        );

        atomic_replace(&path, b"second").expect("file should be replaced");
        assert_eq!(
            std_fs::read(&path).expect("file should be readable"),
            b"second"
        );
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
}
