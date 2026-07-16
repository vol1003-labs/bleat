use std::time::Duration;

use crate::commands::format_messages;
use crate::error::BleatError;
use crate::identity::Role;
use crate::store::Store;

pub fn read<S: Store>(store: &S, role: &Role, peek: bool) -> Result<String, BleatError> {
    let messages = store.read_unread(role, peek)?;
    Ok(format_messages(&messages))
}

pub fn wait<S: Store>(
    store: &S,
    role: &Role,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<Option<String>, BleatError> {
    Ok(store
        .wait_unread(role, timeout, poll_interval)?
        .map(|messages| format_messages(&messages)))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Duration;

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

    #[test]
    fn wait_formats_messages_returned_by_the_store() {
        let store = WaitingStore {
            result: RefCell::new(Some(vec![message(1, "claude", "codex", None, "arrived")])),
        };

        let output = wait(
            &store,
            &role("codex"),
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .expect("wait should succeed")
        .expect("message output should be returned");

        assert_eq!(
            output,
            "---\nid: 0001\nfrom: claude\nto: codex\ntype: report\nts: 2026-07-15T10:00:00+09:00\n---\narrived"
        );
    }

    #[test]
    fn wait_returns_none_when_the_store_times_out() {
        let store = WaitingStore {
            result: RefCell::new(None),
        };

        let output = wait(
            &store,
            &role("codex"),
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .expect("wait should succeed");

        assert!(output.is_none());
    }

    struct TestStore {
        messages: RefCell<Vec<Message>>,
    }

    struct WaitingStore {
        result: RefCell<Option<Vec<Message>>>,
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

        fn wait_unread(
            &self,
            _role: &Role,
            _timeout: Duration,
            _poll_interval: Duration,
        ) -> Result<Option<Vec<Message>>, BleatError> {
            unreachable!("read must not wait for messages")
        }
    }

    impl Store for WaitingStore {
        fn publish(&self, _draft: Draft) -> Result<Message, BleatError> {
            unreachable!("wait must not publish messages")
        }

        fn all(&self) -> Result<Vec<Message>, BleatError> {
            unreachable!("wait must not list all messages")
        }

        fn cursor(&self, _role: &Role) -> Result<u64, BleatError> {
            unreachable!("wait must not inspect the cursor directly")
        }

        fn read_unread(&self, _role: &Role, _peek: bool) -> Result<Vec<Message>, BleatError> {
            unreachable!("wait must not perform a synchronous read")
        }

        fn unread_count(&self, _role: &Role) -> Result<usize, BleatError> {
            unreachable!("wait must not count messages separately")
        }

        fn wait_unread(
            &self,
            _role: &Role,
            _timeout: Duration,
            _poll_interval: Duration,
        ) -> Result<Option<Vec<Message>>, BleatError> {
            Ok(self.result.borrow_mut().take())
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
