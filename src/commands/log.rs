use crate::commands::format_messages;
use crate::error::BleatError;
use crate::store::Store;

pub fn log<S: Store>(store: &S) -> Result<String, BleatError> {
    let messages = store.all()?;
    Ok(format_messages(&messages))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::DateTime;

    use super::*;
    use crate::error::BleatError;
    use crate::identity::{MessageType, Role};
    use crate::message::Message;
    use crate::store::{Draft, Store};

    #[test]
    fn log_returns_every_message_in_sequence_order() {
        let store = TestStore {
            messages: vec![
                message(1, "claude", "codex", None, "first body"),
                message(2, "codex", "claude", None, "second body"),
            ],
        };

        let output = log(&store).expect("log should succeed");

        let ids = output
            .lines()
            .filter_map(|line| line.strip_prefix("id: "))
            .collect::<Vec<_>>();
        assert_eq!(ids, ["0001", "0002"]);
    }

    #[test]
    fn log_preserves_every_message_field_and_the_complete_body() {
        let store = TestStore {
            messages: vec![message(
                2,
                "codex",
                "claude",
                Some(1),
                "first line\nsecond line",
            )],
        };

        let output = log(&store).expect("log should succeed");

        assert_eq!(
            output,
            "---\nid: 0002\nfrom: codex\nto: claude\ntype: report\nre: 0001\nts: 2026-07-15T10:00:00+09:00\n---\nfirst line\nsecond line"
        );
    }

    struct TestStore {
        messages: Vec<Message>,
    }

    impl Store for TestStore {
        fn publish(&self, _draft: Draft) -> Result<Message, BleatError> {
            unreachable!("log must not publish messages")
        }

        fn all(&self) -> Result<Vec<Message>, BleatError> {
            Ok(self.messages.clone())
        }

        fn cursor(&self, _role: &Role) -> Result<u64, BleatError> {
            unreachable!("log must not read cursors")
        }

        fn read_unread(&self, _role: &Role, _peek: bool) -> Result<Vec<Message>, BleatError> {
            unreachable!("log must not consume unread messages")
        }

        fn unread_count(&self, _role: &Role) -> Result<usize, BleatError> {
            unreachable!("log must not count unread messages")
        }

        fn wait_unread(
            &self,
            _role: &Role,
            _timeout: Duration,
            _poll_interval: Duration,
        ) -> Result<Option<Vec<Message>>, BleatError> {
            unreachable!("log must not wait for messages")
        }
    }

    fn message(id: u64, from: &str, to: &str, reply_to: Option<u64>, body: &str) -> Message {
        Message {
            id,
            from: Role::parse(from).expect("from role should be valid"),
            to: Role::parse(to).expect("to role should be valid"),
            kind: MessageType::parse("report").expect("message type should be valid"),
            reply_to,
            timestamp: DateTime::parse_from_rfc3339("2026-07-15T10:00:00+09:00")
                .expect("timestamp should be valid"),
            body: body.to_owned(),
        }
    }
}
