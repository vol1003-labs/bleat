use std::collections::BTreeSet;

use crate::error::BleatError;
use crate::session::Session;
use crate::store::Store;

pub fn status<S: Store>(session: &Session, store: &S) -> Result<String, BleatError> {
    let messages = store.all()?;
    let mut roles = session.roles.keys().cloned().collect::<BTreeSet<_>>();
    for message in &messages {
        roles.insert(message.from.clone());
        roles.insert(message.to.clone());
    }

    let mut output = format!("Session: {}\nRoles:\n", session.slug.as_str());
    for role in roles {
        let registration = if session.roles.contains_key(&role) {
            ""
        } else {
            " (unregistered)"
        };
        let unread = store.unread_count(&role)?;
        output.push_str(&format!(
            "- {}{}: {} unread\n",
            role.as_str(),
            registration,
            unread
        ));
    }
    output.push_str(&format!("Total messages: {}\n", messages.len()));
    match messages.iter().max_by_key(|message| message.id) {
        Some(message) => output.push_str(&format!(
            "Last activity: {} {} -> {}",
            message.timestamp.to_rfc3339(),
            message.from.as_str(),
            message.to.as_str()
        )),
        None => output.push_str("Last activity: none"),
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use chrono::{DateTime, FixedOffset};

    use super::status;
    use crate::error::BleatError;
    use crate::identity::{MessageType, Role, Slug};
    use crate::message::Message;
    use crate::session::{RoleRecord, SESSION_VERSION, Session};
    use crate::store::{Draft, Store};

    #[test]
    fn status_includes_the_session_slug() {
        let output =
            status(&session(&["claude"]), &TestStore::default()).expect("status should render");

        assert!(output.contains("Session: feature"));
    }

    #[test]
    fn status_lists_registered_roles_with_unread_counts() {
        let store = TestStore {
            messages: Vec::new(),
            unread: BTreeMap::from([(role("claude"), 1), (role("codex"), 0)]),
        };

        let output = status(&session(&["claude", "codex"]), &store).expect("status should render");

        assert!(output.contains("- claude: 1 unread\n- codex: 0 unread"));
    }

    #[test]
    fn status_marks_roles_found_only_in_messages_as_unregistered() {
        let store = TestStore {
            messages: vec![message(1, "codex", "reviewer", "2026-07-16T11:00:00+09:00")],
            unread: BTreeMap::from([(role("reviewer"), 1)]),
        };

        let output = status(&session(&["codex"]), &store).expect("status should render");

        assert!(output.contains("- reviewer (unregistered): 1 unread"));
    }

    #[test]
    fn status_reports_the_total_message_count() {
        let store = TestStore {
            messages: vec![
                message(1, "claude", "codex", "2026-07-16T10:00:00+09:00"),
                message(2, "codex", "claude", "2026-07-16T11:00:00+09:00"),
            ],
            unread: BTreeMap::new(),
        };

        let output = status(&session(&["claude", "codex"]), &store).expect("status should render");

        assert!(output.contains("Total messages: 2"));
    }

    #[test]
    fn status_reports_the_latest_message_activity() {
        let store = TestStore {
            messages: vec![
                message(1, "claude", "codex", "2026-07-16T12:00:00+09:00"),
                message(2, "codex", "reviewer", "2026-07-16T11:00:00+09:00"),
            ],
            unread: BTreeMap::new(),
        };

        let output = status(&session(&["claude", "codex"]), &store).expect("status should render");

        assert!(output.contains("Last activity: 2026-07-16T11:00:00+09:00 codex -> reviewer"));
    }

    #[test]
    fn status_reports_no_activity_for_an_empty_log() {
        let output =
            status(&session(&["claude"]), &TestStore::default()).expect("status should render");

        assert!(output.contains("Last activity: none"));
    }

    #[derive(Default)]
    struct TestStore {
        messages: Vec<Message>,
        unread: BTreeMap<Role, usize>,
    }

    impl Store for TestStore {
        fn publish(&self, _draft: Draft) -> Result<Message, BleatError> {
            unreachable!("status must not publish messages")
        }

        fn all(&self) -> Result<Vec<Message>, BleatError> {
            Ok(self.messages.clone())
        }

        fn cursor(&self, _role: &Role) -> Result<u64, BleatError> {
            unreachable!("status must not inspect cursors directly")
        }

        fn read_unread(&self, _role: &Role, _peek: bool) -> Result<Vec<Message>, BleatError> {
            unreachable!("status must not consume unread messages")
        }

        fn unread_count(&self, role: &Role) -> Result<usize, BleatError> {
            Ok(self.unread.get(role).copied().unwrap_or(0))
        }

        fn wait_unread(
            &self,
            _role: &Role,
            _timeout: Duration,
            _poll_interval: Duration,
        ) -> Result<Option<Vec<Message>>, BleatError> {
            unreachable!("status must not wait for messages")
        }
    }

    fn session(role_names: &[&str]) -> Session {
        let created = timestamp("2026-07-16T10:00:00+09:00");
        let roles = role_names
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
            slug: Slug::parse("feature").expect("slug should be valid"),
            created,
            store: "file".to_owned(),
            roles,
            artifacts: serde_json::json!({}),
            extra: BTreeMap::new(),
        }
    }

    fn message(id: u64, from: &str, to: &str, sent: &str) -> Message {
        Message {
            id,
            from: role(from),
            to: role(to),
            kind: MessageType::parse("report").expect("message type should be valid"),
            reply_to: None,
            timestamp: timestamp(sent),
            body: String::new(),
        }
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp should be valid")
    }
}
