use chrono::{DateTime, FixedOffset};

use crate::error::BleatError;
use crate::fs::{SessionLock, atomic_replace};
use crate::identity::Role;
use crate::runtime::{Runtime, RuntimeHandle};
use crate::session::{RoleRecord, SessionPath, encode_session, load_session};

pub fn join(
    session_path: &SessionPath,
    role: Role,
    registered: DateTime<FixedOffset>,
    runtime: &impl Runtime,
) -> Result<(), BleatError> {
    if !session_path.directory().is_dir() || !session_path.session_json().is_file() {
        return Err(BleatError::Usage(format!(
            "session `{}` does not exist",
            session_path.slug().as_str()
        )));
    }
    let handle = runtime.current_handle()?.into_value()?;
    let _lock = SessionLock::acquire(&session_path.directory())?;
    let mut session = load_session(&session_path.session_json())?;

    session
        .roles
        .entry(role.clone())
        .and_modify(|record| record.registered = registered)
        .or_insert_with(|| RoleRecord {
            registered,
            cmd: None,
            extra: Default::default(),
        });
    session.runtime_handles.insert(role, handle);
    atomic_replace(&session_path.session_json(), &encode_session(&session)?)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};

    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use tempfile::tempdir;

    use super::join;
    use crate::error::BleatError;
    use crate::identity::{Role, Slug};
    use crate::runtime::{Runtime, RuntimeHandle};
    use crate::session::{SessionPath, create_session, load_session};

    #[derive(Clone, serde::Deserialize, serde::Serialize)]
    struct FakeHandle {
        channel: String,
        token: u64,
    }

    impl RuntimeHandle for FakeHandle {}

    #[derive(Clone, serde::Deserialize)]
    struct SerializationFailingHandle {
        lock_path: PathBuf,
    }

    impl serde::Serialize for SerializationFailingHandle {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            assert!(!self.lock_path.exists());
            Err(<S::Error as serde::ser::Error>::custom(
                "runtime handle serialization failed",
            ))
        }
    }

    impl RuntimeHandle for SerializationFailingHandle {}

    struct SerializationFailingRuntime {
        lock_path: PathBuf,
    }

    impl Runtime for SerializationFailingRuntime {
        type Handle = SerializationFailingHandle;

        fn current_handle(&self) -> Result<Self::Handle, BleatError> {
            Ok(SerializationFailingHandle {
                lock_path: self.lock_path.clone(),
            })
        }

        fn spawn(
            &self,
            _slug: &Slug,
            _role: &Role,
            _cwd: &Path,
            _argv: &[OsString],
        ) -> Result<Self::Handle, BleatError> {
            unreachable!("join must not spawn a pane")
        }

        fn alive(&self, _handle: &Self::Handle) -> Result<bool, BleatError> {
            unreachable!("join must not inspect pane liveness")
        }

        fn nudge(
            &self,
            _handle: &Self::Handle,
            _slug: &Slug,
            _role: &Role,
        ) -> Result<(), BleatError> {
            unreachable!("join must not nudge a pane")
        }
    }

    struct FakeRuntime {
        result: Result<FakeHandle, String>,
        session_dir: Option<std::path::PathBuf>,
        current_handle_calls: Cell<usize>,
    }

    impl FakeRuntime {
        fn returning(handle: FakeHandle) -> Self {
            Self {
                result: Ok(handle),
                session_dir: None,
                current_handle_calls: Cell::new(0),
            }
        }

        fn failing(message: &str) -> Self {
            Self {
                result: Err(message.to_owned()),
                session_dir: None,
                current_handle_calls: Cell::new(0),
            }
        }

        fn observing_lock(mut self, session_path: &SessionPath) -> Self {
            self.session_dir = Some(session_path.directory());
            self
        }
    }

    impl Runtime for FakeRuntime {
        type Handle = FakeHandle;

        fn current_handle(&self) -> Result<Self::Handle, BleatError> {
            self.current_handle_calls
                .set(self.current_handle_calls.get() + 1);
            if let Some(session_dir) = &self.session_dir {
                assert!(!session_dir.join(".lock").exists());
            }
            self.result
                .as_ref()
                .cloned()
                .map_err(|message| BleatError::Runtime(message.clone()))
        }

        fn spawn(
            &self,
            _slug: &Slug,
            _role: &Role,
            _cwd: &Path,
            _argv: &[OsString],
        ) -> Result<Self::Handle, BleatError> {
            unreachable!("join must not spawn a pane")
        }

        fn alive(&self, _handle: &Self::Handle) -> Result<bool, BleatError> {
            unreachable!("join must not inspect pane liveness")
        }

        fn nudge(
            &self,
            _handle: &Self::Handle,
            _slug: &Slug,
            _role: &Role,
        ) -> Result<(), BleatError> {
            unreachable!("join must not nudge a pane")
        }
    }

    #[test]
    fn join_registers_a_new_role_and_its_current_runtime_handle() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let runtime = FakeRuntime::returning(handle("current", 1));

        join(
            &path,
            role("codex"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect("role should join");

        let persisted = load_session(&path.session_json()).expect("session should load");
        assert!(persisted.roles.contains_key(&role("codex")));
        assert_eq!(
            persisted.runtime_handles[&role("codex")],
            serde_json::json!({
                "channel": "current",
                "token": 1
            })
        );
    }

    #[test]
    fn joining_the_same_role_replaces_its_runtime_handle_without_duplicating_it() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let mut existing = load_session(&path.session_json()).expect("session should load");
        existing.runtime_handles.insert(
            role("claude"),
            serde_json::json!({
                "terminal_id": "term-old",
                "pane_id": "pane-old",
                "agent_name": "stale-agent"
            }),
        );
        fs::write(
            path.session_json(),
            crate::session::encode_session(&existing).expect("session should encode"),
        )
        .expect("fixture should be written");
        let runtime = FakeRuntime::returning(handle("replacement", 2));

        join(
            &path,
            role("claude"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect("role should rejoin");

        let persisted = load_session(&path.session_json()).expect("session should load");
        assert_eq!(persisted.roles.len(), 1);
        assert_eq!(persisted.runtime_handles.len(), 1);
        assert_eq!(
            persisted.runtime_handles[&role("claude")],
            serde_json::json!({
                "channel": "replacement",
                "token": 2
            })
        );
    }

    #[test]
    fn joining_the_same_role_updates_its_registered_timestamp() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let registered = timestamp("2026-07-16T10:00:00+09:00");

        join(
            &path,
            role("claude"),
            registered,
            &FakeRuntime::returning(handle("current", 1)),
        )
        .expect("role should rejoin");

        let persisted = load_session(&path.session_json()).expect("session should load");
        assert_eq!(persisted.roles[&role("claude")].registered, registered);
    }

    #[test]
    fn joining_the_same_role_preserves_unrelated_and_unknown_session_fields() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        fs::write(path.session_json(), session_with_unknown_fields())
            .expect("fixture should be written");

        join(
            &path,
            role("claude"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &FakeRuntime::returning(handle("current", 1)),
        )
        .expect("role should rejoin");

        let bytes = fs::read(path.session_json()).expect("session should be readable");
        let persisted: Value = serde_json::from_slice(&bytes).expect("session should decode");
        assert_eq!(
            persisted["roles"]["claude"]["cmd"],
            serde_json::json!(["claude", "--resume"])
        );
        assert_eq!(persisted["roles"]["claude"]["future_role_field"], 7);
        assert_eq!(persisted["future_top_field"], true);
        assert_eq!(persisted["artifacts"]["spec"], "spec.md");
        assert_eq!(
            persisted["roles"]["codex"],
            serde_json::json!({
                "registered": "2026-07-15T11:00:00+09:00",
                "cmd": ["codex", "--model", "gpt"],
                "future_codex_field": { "enabled": true }
            })
        );
        assert_eq!(
            persisted["runtime_handles"]["codex"],
            serde_json::json!({
                "terminal_id": "term-codex",
                "pane_id": "pane-codex",
                "agent_name": "codex-agent",
                "future_handle_field": 9
            })
        );
    }

    #[test]
    fn joining_a_session_that_no_longer_exists_skips_runtime_io_and_reports_usage() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        fs::remove_dir_all(path.directory()).expect("session should be removed");
        let runtime = FakeRuntime::failing("runtime error must not mask missing session");

        let error = join(
            &path,
            role("codex"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect_err("missing session should fail");

        assert!(matches!(error, BleatError::Usage(_)));
        assert!(error.to_string().contains("feature"));
        assert_eq!(runtime.current_handle_calls.get(), 0);
    }

    #[test]
    fn runtime_failure_leaves_the_session_file_byte_for_byte_unchanged() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let before = fs::read(path.session_json()).expect("session should be readable");

        let error = join(
            &path,
            role("codex"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &FakeRuntime::failing("current pane unavailable"),
        )
        .expect_err("runtime failure should fail join");

        assert!(matches!(error, BleatError::Runtime(_)));
        assert_eq!(
            fs::read(path.session_json()).expect("session should remain"),
            before
        );
    }

    #[test]
    fn serialization_failure_precedes_locking_and_leaves_the_session_unchanged() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let before = fs::read(path.session_json()).expect("session should be readable");
        let lock_path = path.directory().join(".lock");

        let error = join(
            &path,
            role("codex"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &SerializationFailingRuntime {
                lock_path: lock_path.clone(),
            },
        )
        .expect_err("serialization failure should fail join");

        assert!(
            matches!(error, BleatError::Runtime(message) if message.contains("encode runtime handle") && message.contains("runtime handle serialization failed"))
        );
        assert!(!lock_path.exists());
        assert_eq!(
            fs::read(path.session_json()).expect("session should remain readable"),
            before
        );
    }

    #[test]
    fn join_gets_the_current_handle_before_acquiring_the_session_lock() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let runtime = FakeRuntime::returning(handle("current", 1)).observing_lock(&path);

        join(
            &path,
            role("codex"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect("role should join");

        assert_eq!(runtime.current_handle_calls.get(), 1);
    }

    fn session(root: &Path) -> SessionPath {
        create_session(
            root,
            Slug::parse("feature").expect("slug should be valid"),
            role("claude"),
            timestamp("2026-07-15T10:00:00+09:00"),
        )
        .expect("session should be created")
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp should be valid")
    }

    fn handle(channel: &str, token: u64) -> FakeHandle {
        FakeHandle {
            channel: channel.to_owned(),
            token,
        }
    }

    fn session_with_unknown_fields() -> &'static str {
        r#"{
          "version": 1,
          "slug": "feature",
          "created": "2026-07-15T10:00:00+09:00",
          "store": "file",
          "runtime": "herdr",
          "roles": {
            "claude": {
              "registered": "2026-07-15T10:00:00+09:00",
              "cmd": ["claude", "--resume"],
              "future_role_field": 7
            },
            "codex": {
              "registered": "2026-07-15T11:00:00+09:00",
              "cmd": ["codex", "--model", "gpt"],
              "future_codex_field": { "enabled": true }
            }
          },
          "runtime_handles": {
            "claude": { "terminal_id": "term-old", "pane_id": "pane-old" },
            "codex": {
              "terminal_id": "term-codex",
              "pane_id": "pane-codex",
              "agent_name": "codex-agent",
              "future_handle_field": 9
            }
          },
          "artifacts": { "spec": "spec.md" },
          "future_top_field": true
        }"#
    }
}
