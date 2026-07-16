use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::error::BleatError;
use crate::fs::{SessionLock, atomic_replace, cleanup_old_message_temps};
use crate::identity::Role;
use crate::message::{Message, decode, encode};
use crate::store::{Draft, Store};

pub struct FileStore {
    session_dir: PathBuf,
}

impl FileStore {
    pub fn new(session_dir: PathBuf) -> Self {
        Self { session_dir }
    }

    fn cursor_path(&self, role: &Role) -> PathBuf {
        self.session_dir
            .join("messages")
            .join(format!(".cursor-{}", role.as_str()))
    }

    fn unread_after(&self, role: &Role, cursor: u64) -> Result<Vec<Message>, BleatError> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|message| &message.to == role && message.id > cursor)
            .collect())
    }

    fn write_cursor(&self, role: &Role, cursor: u64) -> Result<(), BleatError> {
        atomic_replace(
            self.cursor_path(role).as_path(),
            format!("{cursor}\n").as_bytes(),
        )
    }
}

impl Store for FileStore {
    fn publish(&self, draft: Draft) -> Result<Message, BleatError> {
        let _lock = SessionLock::acquire(&self.session_dir)?;
        let messages_dir = self.session_dir.join("messages");
        cleanup_old_message_temps(&messages_dir, SystemTime::now())?;
        let id = next_message_id(&messages_dir)?;
        let message = Message {
            id,
            from: draft.from,
            to: draft.to,
            kind: draft.kind,
            reply_to: draft.reply_to,
            timestamp: draft.timestamp,
            body: draft.body,
        };
        let path = messages_dir.join(format!("{id:04}.md"));
        atomic_replace(&path, encode(&message).as_bytes())?;
        Ok(message)
    }

    fn all(&self) -> Result<Vec<Message>, BleatError> {
        let messages_dir = self.session_dir.join("messages");
        let mut messages = Vec::new();
        for (id, path) in message_paths(&messages_dir)? {
            let source = fs::read_to_string(&path).map_err(|source| {
                BleatError::Execution(format!(
                    "failed to read message `{}`: {source}",
                    path.display()
                ))
            })?;
            messages.push(decode(id, &source)?);
        }
        messages.sort_by_key(|message| message.id);
        Ok(messages)
    }

    fn cursor(&self, role: &Role) -> Result<u64, BleatError> {
        let path = self.cursor_path(role);
        match fs::read_to_string(&path) {
            Ok(value) => value
                .trim()
                .parse()
                .map_err(|_| BleatError::Execution(format!("invalid cursor `{}`", path.display()))),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(source) => Err(BleatError::Execution(format!(
                "failed to read cursor `{}`: {source}",
                path.display()
            ))),
        }
    }

    fn read_unread(&self, role: &Role, peek: bool) -> Result<Vec<Message>, BleatError> {
        if peek {
            let cursor = self.cursor(role)?;
            return self.unread_after(role, cursor);
        }

        let _lock = SessionLock::acquire(&self.session_dir)?;
        let cursor = self.cursor(role)?;
        let messages = self.unread_after(role, cursor)?;
        if let Some(delivered) = messages.iter().map(|message| message.id).max() {
            self.write_cursor(role, cursor.max(delivered))?;
        }
        Ok(messages)
    }

    fn unread_count(&self, role: &Role) -> Result<usize, BleatError> {
        let cursor = self.cursor(role)?;
        Ok(self.unread_after(role, cursor)?.len())
    }

    fn wait_unread(
        &self,
        role: &Role,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Option<Vec<Message>>, BleatError> {
        let started = Instant::now();
        loop {
            let messages = self.read_unread(role, false)?;
            if !messages.is_empty() {
                return Ok(Some(messages));
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Ok(None);
            }
            thread::sleep(poll_interval.min(timeout - elapsed));
        }
    }
}

fn next_message_id(messages_dir: &Path) -> Result<u64, BleatError> {
    let largest = message_paths(messages_dir)?
        .into_iter()
        .map(|(id, _)| id)
        .max()
        .unwrap_or(0);
    largest
        .checked_add(1)
        .ok_or_else(|| BleatError::Execution("message id overflow".to_owned()))
}

fn message_paths(messages_dir: &Path) -> Result<Vec<(u64, PathBuf)>, BleatError> {
    let entries = fs::read_dir(messages_dir).map_err(|source| {
        BleatError::Execution(format!(
            "failed to read messages directory `{}`: {source}",
            messages_dir.display()
        ))
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| {
            BleatError::Execution(format!("failed to read message entry: {source}"))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            BleatError::Execution(format!(
                "invalid message filename `{}`",
                entry.path().display()
            ))
        })?;
        if name.starts_with(".tmp-") || name.starts_with(".cursor-") {
            continue;
        }
        let id = name
            .strip_suffix(".md")
            .and_then(|stem| stem.parse::<u64>().ok())
            .filter(|id| *id > 0 && format!("{id:04}.md") == name)
            .ok_or_else(|| BleatError::Execution(format!("invalid message filename `{name}`")))?;
        paths.push((id, entry.path()));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use chrono::DateTime;
    use tempfile::tempdir;

    use super::*;
    use crate::identity::{MessageType, Role};

    #[test]
    fn publish_assigns_id_one_in_an_empty_store() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let message = store
            .publish(draft("first"))
            .expect("message should publish");

        assert_eq!(message.id, 1);
    }

    #[test]
    fn publish_persists_the_message_in_the_messages_directory() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let message = store
            .publish(draft("first"))
            .expect("message should publish");

        let persisted = fs::read_to_string(sandbox.path().join("messages/0001.md"))
            .expect("persisted message should be readable");
        assert_eq!(persisted, encode(&message));
    }

    #[test]
    fn publish_assigns_one_more_than_the_largest_existing_id() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("first"))
            .expect("first message should publish");

        let message = store
            .publish(draft("second"))
            .expect("second message should publish");

        assert_eq!(message.id, 2);
    }

    #[test]
    fn publish_grows_naturally_beyond_four_digits() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        fs::write(messages_dir.join("9999.md"), "existing")
            .expect("existing message should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let message = store
            .publish(draft("next"))
            .expect("message should publish");

        assert_eq!(message.id, 10_000);
    }

    #[test]
    fn concurrent_publish_assigns_distinct_dense_ids() {
        const PUBLISHERS: usize = 8;

        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = Arc::new(FileStore::new(sandbox.path().to_path_buf()));
        let barrier = Arc::new(Barrier::new(PUBLISHERS));
        let handles = (0..PUBLISHERS)
            .map(|index| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    store.publish(draft(&format!("message {index}")))
                })
            })
            .collect::<Vec<_>>();

        let mut ids = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .expect("publisher should not panic")
                    .expect("message should publish")
                    .id
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();

        assert_eq!(ids, (1..=PUBLISHERS as u64).collect::<Vec<_>>());
    }

    #[test]
    fn all_returns_messages_in_numeric_id_order() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        write_message(&messages_dir, 10, "tenth");
        write_message(&messages_dir, 2, "second");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let messages = store.all().expect("messages should load");

        assert_eq!(
            messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>(),
            vec![2, 10]
        );
    }

    #[test]
    fn all_rejects_a_malformed_message_filename() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        fs::write(messages_dir.join("broken.md"), "broken")
            .expect("broken message should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let error = store.all().expect_err("malformed filename should fail");

        assert!(matches!(error, BleatError::Execution(_)));
        assert!(error.to_string().contains("invalid message filename"));
    }

    #[test]
    fn all_rejects_malformed_message_contents() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        fs::write(messages_dir.join("0001.md"), "broken")
            .expect("broken message should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let error = store.all().expect_err("malformed message should fail");

        assert!(matches!(error, BleatError::Execution(_)));
        assert!(error.to_string().contains("failed to decode message"));
    }

    #[test]
    fn read_unread_returns_only_messages_addressed_to_the_role() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft_to("for claude", "claude"))
            .expect("claude message should publish");
        store
            .publish(draft_to("for codex", "codex"))
            .expect("codex message should publish");

        let messages = store
            .read_unread(&Role::parse("claude").expect("role should be valid"), true)
            .expect("unread messages should load");

        assert_eq!(
            messages
                .iter()
                .map(|message| message.body.as_str())
                .collect::<Vec<_>>(),
            vec!["for claude"]
        );
    }

    #[test]
    fn cursor_defaults_to_zero_when_the_role_has_not_read_messages() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        let role = Role::parse("claude").expect("role should be valid");

        let cursor = store.cursor(&role).expect("cursor should load");

        assert_eq!(cursor, 0);
    }

    #[test]
    fn cursor_reads_the_roles_saved_position() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        fs::write(messages_dir.join(".cursor-claude"), "7\n").expect("cursor should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());
        let role = Role::parse("claude").expect("role should be valid");

        let cursor = store.cursor(&role).expect("cursor should load");

        assert_eq!(cursor, 7);
    }

    #[test]
    fn read_unread_returns_only_messages_after_the_cursor() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("first"))
            .expect("first message should publish");
        store
            .publish(draft("second"))
            .expect("second message should publish");
        fs::write(messages_dir.join(".cursor-claude"), "1\n").expect("cursor should be written");
        let role = Role::parse("claude").expect("role should be valid");

        let messages = store
            .read_unread(&role, true)
            .expect("unread messages should load");

        assert_eq!(
            messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn peek_does_not_advance_the_cursor() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("first"))
            .expect("message should publish");
        let role = Role::parse("claude").expect("role should be valid");

        store
            .read_unread(&role, true)
            .expect("peek should load messages");

        assert_eq!(store.cursor(&role).expect("cursor should load"), 0);
    }

    #[test]
    fn read_advances_the_cursor_to_the_largest_delivered_id() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("first"))
            .expect("first message should publish");
        store
            .publish(draft("second"))
            .expect("second message should publish");
        let role = Role::parse("claude").expect("role should be valid");

        store
            .read_unread(&role, false)
            .expect("read should load messages");

        assert_eq!(store.cursor(&role).expect("cursor should load"), 2);
    }

    #[test]
    fn read_does_not_move_the_cursor_backward() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        write_message(&messages_dir, 1, "first");
        fs::write(messages_dir.join(".cursor-claude"), "7\n").expect("cursor should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());
        let role = Role::parse("claude").expect("role should be valid");

        store
            .read_unread(&role, false)
            .expect("read should succeed");

        assert_eq!(store.cursor(&role).expect("cursor should load"), 7);
    }

    #[test]
    fn cursor_rejects_a_non_numeric_position() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        fs::write(messages_dir.join(".cursor-claude"), "broken\n")
            .expect("cursor should be written");
        let store = FileStore::new(sandbox.path().to_path_buf());
        let role = Role::parse("claude").expect("role should be valid");

        let error = store.cursor(&role).expect_err("broken cursor should fail");

        assert!(matches!(error, BleatError::Execution(_)));
        assert!(error.to_string().contains("invalid cursor"));
    }

    #[test]
    fn concurrent_read_delivers_each_unread_message_once_per_role() {
        const READERS: usize = 8;

        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = Arc::new(FileStore::new(sandbox.path().to_path_buf()));
        store
            .publish(draft("first"))
            .expect("first message should publish");
        store
            .publish(draft("second"))
            .expect("second message should publish");
        let barrier = Arc::new(Barrier::new(READERS));
        let handles = (0..READERS)
            .map(|_| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let role = Role::parse("claude").expect("role should be valid");
                    barrier.wait();
                    store.read_unread(&role, false)
                })
            })
            .collect::<Vec<_>>();

        let mut delivered_counts = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .expect("reader should not panic")
                    .expect("read should succeed")
                    .len()
            })
            .collect::<Vec<_>>();
        delivered_counts.sort_unstable();

        assert_eq!(delivered_counts, vec![0, 0, 0, 0, 0, 0, 0, 2]);
    }

    #[test]
    fn unread_count_reports_the_roles_remaining_messages() {
        let sandbox = tempdir().expect("sandbox should be created");
        let messages_dir = sandbox.path().join("messages");
        fs::create_dir(&messages_dir).expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("already read"))
            .expect("first message should publish");
        store
            .publish(draft_to("for codex", "codex"))
            .expect("codex message should publish");
        store
            .publish(draft("still unread"))
            .expect("last message should publish");
        fs::write(messages_dir.join(".cursor-claude"), "1\n").expect("cursor should be written");
        let role = Role::parse("claude").expect("role should be valid");

        let count = store.unread_count(&role).expect("unread count should load");

        assert_eq!(count, 1);
    }

    #[test]
    fn wait_unread_returns_existing_messages_immediately() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());
        store
            .publish(draft("already here"))
            .expect("message should publish");

        let messages = store
            .wait_unread(
                &role("claude"),
                Duration::from_secs(1),
                Duration::from_millis(5),
            )
            .expect("wait should succeed")
            .expect("existing unread should be returned");

        assert_eq!(messages[0].body, "already here");
    }

    #[test]
    fn wait_unread_returns_a_message_published_while_waiting() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = Arc::new(FileStore::new(sandbox.path().to_path_buf()));
        let waiting = Arc::clone(&store);
        let barrier = Arc::new(Barrier::new(2));
        let waiting_barrier = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            waiting_barrier.wait();
            waiting.wait_unread(
                &role("claude"),
                Duration::from_secs(1),
                Duration::from_millis(5),
            )
        });
        barrier.wait();
        thread::sleep(Duration::from_millis(20));
        store
            .publish(draft("arrived later"))
            .expect("message should publish");

        let messages = handle
            .join()
            .expect("waiter should not panic")
            .expect("wait should succeed")
            .expect("new unread should be returned");

        assert_eq!(messages[0].body, "arrived later");
    }

    #[test]
    fn wait_unread_returns_none_after_the_timeout() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir(sandbox.path().join("messages"))
            .expect("messages directory should be created");
        let store = FileStore::new(sandbox.path().to_path_buf());

        let result = store
            .wait_unread(
                &role("claude"),
                Duration::from_millis(20),
                Duration::from_millis(5),
            )
            .expect("wait should succeed");

        assert!(result.is_none());
    }

    fn draft(body: &str) -> Draft {
        draft_to(body, "claude")
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }

    fn draft_to(body: &str, to: &str) -> Draft {
        Draft {
            from: Role::parse("codex").expect("from role should be valid"),
            to: Role::parse(to).expect("to role should be valid"),
            kind: MessageType::parse("report").expect("message type should be valid"),
            reply_to: None,
            timestamp: DateTime::parse_from_rfc3339("2026-07-15T10:00:00+09:00")
                .expect("timestamp should be valid"),
            body: body.to_owned(),
        }
    }

    fn write_message(messages_dir: &std::path::Path, id: u64, body: &str) {
        let draft = draft(body);
        let message = Message {
            id,
            from: draft.from,
            to: draft.to,
            kind: draft.kind,
            reply_to: draft.reply_to,
            timestamp: draft.timestamp,
            body: draft.body,
        };
        fs::write(messages_dir.join(format!("{id:04}.md")), encode(&message))
            .expect("message fixture should be written");
    }
}
