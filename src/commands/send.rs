use std::fs;
use std::io::Read;
use std::path::Path;

use chrono::Utc;

use crate::error::BleatError;
use crate::identity::{MessageType, Role};
use crate::message::Message;
use crate::session::Session;
use crate::store::{Draft, Store};

pub fn read_body<R: Read>(
    positional: Option<String>,
    file: Option<&Path>,
    stdin: &mut R,
    stdin_is_tty: bool,
) -> Result<String, BleatError> {
    if positional.is_some() && file.is_some() {
        return Err(BleatError::Usage(
            "message body must use either positional text or --file, not both".to_owned(),
        ));
    }
    if let Some(body) = positional {
        return Ok(body);
    }
    if let Some(path) = file {
        if path == Path::new("-") {
            return read_stdin(stdin);
        }
        return fs::read_to_string(path).map_err(|source| {
            BleatError::Execution(format!(
                "failed to read message body `{}`: {source}",
                path.display()
            ))
        });
    }
    if !stdin_is_tty {
        return read_stdin(stdin);
    }

    Err(BleatError::Usage(
        "message body is required as positional text, --file, or stdin".to_owned(),
    ))
}

fn read_stdin<R: Read>(stdin: &mut R) -> Result<String, BleatError> {
    let mut body = String::new();
    stdin.read_to_string(&mut body).map_err(|source| {
        BleatError::Execution(format!("failed to read message body from stdin: {source}"))
    })?;
    Ok(body)
}

pub fn send<S: Store>(
    session: &Session,
    from: Role,
    to: Role,
    kind: Option<MessageType>,
    reply_to: Option<u64>,
    body: String,
    store: &S,
) -> Result<Message, BleatError> {
    if !session.roles.contains_key(&from) {
        return Err(BleatError::Usage(format!(
            "sender role `{}` is not registered in session `{}`",
            from.as_str(),
            session.slug.as_str()
        )));
    }
    if !session.roles.contains_key(&to) {
        return Err(BleatError::Usage(format!(
            "recipient role `{}` is not registered in session `{}`",
            to.as_str(),
            session.slug.as_str()
        )));
    }
    if matches!(reply_to, Some(0)) {
        return Err(BleatError::Usage(
            "reply id must be greater than zero".to_owned(),
        ));
    }
    let kind = kind.map_or_else(|| MessageType::parse("report"), Ok)?;
    store.publish(Draft {
        from,
        to,
        kind,
        reply_to,
        timestamp: Utc::now().fixed_offset(),
        body,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Cursor;

    use chrono::DateTime;
    use tempfile::tempdir;

    use super::*;
    use crate::session::{RoleRecord, SESSION_VERSION};

    #[test]
    fn read_body_uses_the_positional_body() {
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let body = read_body(Some("hello".to_owned()), None, &mut stdin, true)
            .expect("positional body should be accepted");

        assert_eq!(body, "hello");
    }

    #[test]
    fn read_body_rejects_positional_and_file_inputs_together() {
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let error = read_body(
            Some("hello".to_owned()),
            Some(Path::new("message.md")),
            &mut stdin,
            true,
        )
        .expect_err("multiple body sources should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }

    #[test]
    fn read_body_reads_the_file_body() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("message.md");
        fs::write(&path, "from a file").expect("message file should be written");
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let body =
            read_body(None, Some(&path), &mut stdin, true).expect("file body should be accepted");

        assert_eq!(body, "from a file");
    }

    #[test]
    fn read_body_reads_non_tty_stdin_when_no_source_is_given() {
        let mut stdin = Cursor::new(b"from stdin".to_vec());

        let body = read_body(None, None, &mut stdin, false)
            .expect("non-TTY stdin body should be accepted");

        assert_eq!(body, "from stdin");
    }

    #[test]
    fn read_body_treats_file_dash_as_stdin() {
        let mut stdin = Cursor::new(b"from explicit stdin".to_vec());

        let body = read_body(None, Some(Path::new("-")), &mut stdin, true)
            .expect("--file - should read stdin");

        assert_eq!(body, "from explicit stdin");
    }

    #[test]
    fn read_body_rejects_missing_input_from_a_tty() {
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let error =
            read_body(None, None, &mut stdin, true).expect_err("missing TTY input should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }

    #[test]
    fn read_body_accepts_an_empty_positional_body() {
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let body = read_body(Some(String::new()), None, &mut stdin, true)
            .expect("empty body should be accepted");

        assert_eq!(body, "");
    }

    #[test]
    fn read_body_accepts_an_empty_file_body() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("message.md");
        fs::write(&path, "").expect("empty message file should be written");
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let body =
            read_body(None, Some(&path), &mut stdin, true).expect("empty file should be accepted");

        assert_eq!(body, "");
    }

    #[test]
    fn read_body_accepts_empty_non_tty_stdin() {
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let body = read_body(None, None, &mut stdin, false)
            .expect("empty non-TTY stdin should be accepted");

        assert_eq!(body, "");
    }

    #[test]
    fn read_body_rejects_non_utf8_file_contents() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("message.md");
        fs::write(&path, [0xff]).expect("message file should be written");
        let mut stdin = Cursor::new(Vec::<u8>::new());

        let error = read_body(None, Some(&path), &mut stdin, true)
            .expect_err("non-UTF-8 file body should fail");

        assert!(matches!(error, BleatError::Execution(_)));
    }

    #[test]
    fn read_body_rejects_non_utf8_stdin() {
        let mut stdin = Cursor::new(vec![0xff]);

        let error =
            read_body(None, None, &mut stdin, false).expect_err("non-UTF-8 stdin body should fail");

        assert!(matches!(error, BleatError::Execution(_)));
    }

    #[test]
    fn send_publishes_the_message() {
        let session = session_with_roles(&["codex", "claude"]);
        let store = RecordingStore::default();

        let message = send(
            &session,
            role("codex"),
            role("claude"),
            Some(MessageType::parse("question").expect("message type should be valid")),
            Some(7),
            "Can you review this?".to_owned(),
            &store,
        )
        .expect("registered roles should send a message");

        assert_eq!(store.publish_calls.get(), 1);
        assert_eq!(message.from, role("codex"));
        assert_eq!(message.to, role("claude"));
        assert_eq!(message.kind.as_str(), "question");
        assert_eq!(message.reply_to, Some(7));
        assert_eq!(message.body, "Can you review this?");
    }

    #[test]
    fn send_defaults_the_message_type_to_report() {
        let session = session_with_roles(&["codex", "claude"]);
        let store = RecordingStore::default();

        let message = send(
            &session,
            role("codex"),
            role("claude"),
            None,
            None,
            "Finished the task.".to_owned(),
            &store,
        )
        .expect("default message type should be accepted");

        assert_eq!(message.kind.as_str(), "report");
    }

    #[test]
    fn send_rejects_an_unregistered_sender() {
        let session = session_with_roles(&["claude"]);
        let store = RecordingStore::default();

        let error = send(
            &session,
            role("codex"),
            role("claude"),
            None,
            None,
            "hello".to_owned(),
            &store,
        )
        .expect_err("unregistered sender should fail");

        assert!(matches!(error, BleatError::Usage(_)));
        assert_eq!(store.publish_calls.get(), 0);
    }

    #[test]
    fn send_rejects_an_unregistered_recipient() {
        let session = session_with_roles(&["codex"]);
        let store = RecordingStore::default();

        let error = send(
            &session,
            role("codex"),
            role("claude"),
            None,
            None,
            "hello".to_owned(),
            &store,
        )
        .expect_err("unregistered recipient should fail");

        assert!(matches!(error, BleatError::Usage(_)));
        assert_eq!(store.publish_calls.get(), 0);
    }

    #[test]
    fn send_rejects_reply_id_zero() {
        let session = session_with_roles(&["codex", "claude"]);
        let store = RecordingStore::default();

        let error = send(
            &session,
            role("codex"),
            role("claude"),
            None,
            Some(0),
            "hello".to_owned(),
            &store,
        )
        .expect_err("reply id zero should fail");

        assert!(matches!(error, BleatError::Usage(_)));
        assert_eq!(store.publish_calls.get(), 0);
    }

    #[derive(Default)]
    struct RecordingStore {
        publish_calls: Cell<usize>,
    }

    impl Store for RecordingStore {
        fn publish(&self, draft: Draft) -> Result<Message, BleatError> {
            self.publish_calls.set(self.publish_calls.get() + 1);
            Ok(Message {
                id: 1,
                from: draft.from,
                to: draft.to,
                kind: draft.kind,
                reply_to: draft.reply_to,
                timestamp: draft.timestamp,
                body: draft.body,
            })
        }

        fn all(&self) -> Result<Vec<Message>, BleatError> {
            unreachable!("send must not list messages")
        }

        fn cursor(&self, _role: &Role) -> Result<u64, BleatError> {
            unreachable!("send must not read a cursor")
        }

        fn read_unread(&self, _role: &Role, _peek: bool) -> Result<Vec<Message>, BleatError> {
            unreachable!("send must not read messages")
        }

        fn unread_count(&self, _role: &Role) -> Result<usize, BleatError> {
            unreachable!("send must not count messages")
        }
    }

    fn session_with_roles(names: &[&str]) -> Session {
        let created = DateTime::parse_from_rfc3339("2026-07-15T10:00:00+09:00")
            .expect("timestamp should be valid");
        let roles = names
            .iter()
            .map(|name| {
                (
                    role(name),
                    RoleRecord {
                        registered: created,
                        extra: BTreeMap::new(),
                    },
                )
            })
            .collect();
        Session {
            version: SESSION_VERSION,
            slug: crate::identity::Slug::parse("session").expect("slug should be valid"),
            created,
            store: "file".to_owned(),
            roles,
            artifacts: serde_json::json!({}),
            extra: BTreeMap::new(),
        }
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }
}
