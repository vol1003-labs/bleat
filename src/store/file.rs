use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::BleatError;
use crate::fs::{SessionLock, atomic_replace, cleanup_old_message_temps};
use crate::message::{Message, decode, encode};
use crate::store::{Draft, Store};

pub struct FileStore {
    session_dir: PathBuf,
}

impl FileStore {
    pub fn new(session_dir: PathBuf) -> Self {
        Self { session_dir }
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
                BleatError::Runtime(format!(
                    "failed to read message `{}`: {source}",
                    path.display()
                ))
            })?;
            messages.push(decode(id, &source)?);
        }
        messages.sort_by_key(|message| message.id);
        Ok(messages)
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
        .ok_or_else(|| BleatError::Runtime("message id overflow".to_owned()))
}

fn message_paths(messages_dir: &Path) -> Result<Vec<(u64, PathBuf)>, BleatError> {
    let entries = fs::read_dir(messages_dir).map_err(|source| {
        BleatError::Runtime(format!(
            "failed to read messages directory `{}`: {source}",
            messages_dir.display()
        ))
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| {
            BleatError::Runtime(format!("failed to read message entry: {source}"))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            BleatError::Runtime(format!(
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
            .ok_or_else(|| BleatError::Runtime(format!("invalid message filename `{name}`")))?;
        paths.push((id, entry.path()));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

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

        assert!(matches!(error, BleatError::Runtime(_)));
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

        assert!(matches!(error, BleatError::Runtime(_)));
        assert!(error.to_string().contains("failed to decode message"));
    }

    fn draft(body: &str) -> Draft {
        Draft {
            from: Role::parse("codex").expect("from role should be valid"),
            to: Role::parse("claude").expect("to role should be valid"),
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
