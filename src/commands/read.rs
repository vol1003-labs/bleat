use crate::commands::format_messages;
use crate::error::BleatError;
use crate::identity::Role;
use crate::store::Store;

pub fn read<S: Store>(store: &S, role: &Role, peek: bool) -> Result<String, BleatError> {
    let messages = store.read_unread(role, peek)?;
    Ok(format_messages(&messages))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::DateTime;

    use super::*;
    use crate::error::BleatError;
    use crate::identity::{MessageType, Role};
    use crate::message::Message;
    use crate::store::{Draft, Store};

    #[test]
    fn read_returns_empty_output_when_there_are_no_unread_messages() {
        let store = TestStore::new(Vec::new());

        let output = read(&store, &role("codex"), false).expect("read should succeed");

        assert_eq!(output, "");
    }

    #[test]
    fn read_returns_only_messages_addressed_to_the_role() {
        let store = TestStore::new(vec![
            message(1, "claude", "codex", None, "for codex"),
            message(2, "codex", "claude", None, "for claude"),
        ]);

        let output = read(&store, &role("codex"), false).expect("read should succeed");

        assert_eq!(
            output,
            "---\nid: 0001\nfrom: claude\nto: codex\ntype: report\nts: 2026-07-15T10:00:00+09:00\n---\nfor codex"
        );
    }

    #[test]
    fn peek_allows_the_same_unread_message_to_be_read_again() {
        let store = TestStore::new(vec![message(
            1,
            "claude",
            "codex",
            Some(7),
            "please review",
        )]);

        let first = read(&store, &role("codex"), true).expect("first peek should succeed");
        let second = read(&store, &role("codex"), true).expect("second peek should succeed");

        assert_eq!(second, first);
    }

    struct TestStore {
        messages: RefCell<Vec<Message>>,
    }

    impl TestStore {
        fn new(messages: Vec<Message>) -> Self {
            Self {
                messages: RefCell::new(messages),
            }
        }
    }

    impl Store for TestStore {
        fn publish(&self, _draft: Draft) -> Result<Message, BleatError> {
            unreachable!("read must not publish messages")
        }

        fn all(&self) -> Result<Vec<Message>, BleatError> {
            unreachable!("read must not list all messages")
        }

        fn cursor(&self, _role: &Role) -> Result<u64, BleatError> {
            unreachable!("read must not inspect the cursor directly")
        }

        fn read_unread(&self, role: &Role, peek: bool) -> Result<Vec<Message>, BleatError> {
            let messages = self
                .messages
                .borrow()
                .iter()
                .filter(|message| &message.to == role)
                .cloned()
                .collect();
            if !peek {
                self.messages
                    .borrow_mut()
                    .retain(|message| &message.to != role);
            }
            Ok(messages)
        }

        fn unread_count(&self, _role: &Role) -> Result<usize, BleatError> {
            unreachable!("read must not count messages separately")
        }
    }

    fn message(id: u64, from: &str, to: &str, reply_to: Option<u64>, body: &str) -> Message {
        Message {
            id,
            from: role(from),
            to: role(to),
            kind: MessageType::parse("report").expect("message type should be valid"),
            reply_to,
            timestamp: DateTime::parse_from_rfc3339("2026-07-15T10:00:00+09:00")
                .expect("timestamp should be valid"),
            body: body.to_owned(),
        }
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }
}
