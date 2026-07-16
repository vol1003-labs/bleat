use std::path::Path;

use chrono::{DateTime, FixedOffset};

use crate::error::BleatError;
use crate::identity::{Role, Slug};
use crate::runtime::Runtime;
use crate::session::{SessionPath, create_session_with_handle};

pub fn init(
    root: &Path,
    slug: Slug,
    created: DateTime<FixedOffset>,
    runtime: &impl Runtime,
) -> Result<SessionPath, BleatError> {
    let handle = runtime.current_handle()?;
    create_session_with_handle(root, slug, Role::parse("claude")?, created, handle)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ffi::OsString;
    use std::path::Path;

    use chrono::{DateTime, FixedOffset};
    use tempfile::tempdir;

    use super::init;
    use crate::error::BleatError;
    use crate::identity::{Role, Slug};
    use crate::runtime::{Runtime, RuntimeHandle};
    use crate::session::load_session;

    #[derive(Clone, serde::Deserialize, serde::Serialize)]
    struct FakeHandle {
        channel: String,
        token: u64,
    }

    impl RuntimeHandle for FakeHandle {}

    struct FakeRuntime {
        result: Result<FakeHandle, String>,
        bleat_root: std::path::PathBuf,
        current_handle_calls: Cell<usize>,
    }

    impl Runtime for FakeRuntime {
        type Handle = FakeHandle;

        fn current_handle(&self) -> Result<Self::Handle, BleatError> {
            self.current_handle_calls
                .set(self.current_handle_calls.get() + 1);
            assert!(!self.bleat_root.exists());
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
            unreachable!("init must not spawn a pane")
        }

        fn alive(&self, _handle: &Self::Handle) -> Result<bool, BleatError> {
            unreachable!("init must not inspect pane liveness")
        }

        fn nudge(
            &self,
            _handle: &Self::Handle,
            _slug: &Slug,
            _role: &Role,
        ) -> Result<(), BleatError> {
            unreachable!("init must not nudge a pane")
        }
    }

    #[test]
    fn init_registers_claude_with_the_current_runtime_handle() {
        let sandbox = tempdir().expect("sandbox should be created");
        let runtime = runtime(
            sandbox.path(),
            Ok(FakeHandle {
                channel: "current".to_owned(),
                token: 1,
            }),
        );

        let path = init(
            sandbox.path(),
            Slug::parse("feature").expect("slug should be valid"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect("session should initialize");

        let session = load_session(&path.session_json()).expect("session should load");
        let claude = Role::parse("claude").expect("role should be valid");
        assert_eq!(
            session.roles[&claude].registered,
            timestamp("2026-07-16T10:00:00+09:00")
        );
        assert_eq!(
            session.runtime_handles[&claude],
            serde_json::json!({ "channel": "current", "token": 1 })
        );
        assert_eq!(runtime.current_handle_calls.get(), 1);
    }

    #[test]
    fn runtime_failure_does_not_create_an_incomplete_session() {
        let sandbox = tempdir().expect("sandbox should be created");
        let runtime = runtime(sandbox.path(), Err("current pane unavailable".to_owned()));

        let error = init(
            sandbox.path(),
            Slug::parse("feature").expect("slug should be valid"),
            timestamp("2026-07-16T10:00:00+09:00"),
            &runtime,
        )
        .expect_err("runtime failure should fail init");

        assert!(matches!(error, BleatError::Runtime(_)));
        assert!(!sandbox.path().join(".bleat").exists());
    }

    fn runtime(root: &Path, result: Result<FakeHandle, String>) -> FakeRuntime {
        FakeRuntime {
            result,
            bleat_root: root.join(".bleat"),
            current_handle_calls: Cell::new(0),
        }
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp should be valid")
    }
}
