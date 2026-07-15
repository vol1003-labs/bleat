use std::ffi::OsString;
use std::path::Path;

use crate::error::BleatError;
use crate::identity::{Role, Slug};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeHandle {
    pub terminal_id: String,
    pub pane_id: String,
    pub agent_name: Option<String>,
}

pub trait Runtime {
    fn current_handle(&self) -> Result<RuntimeHandle, BleatError>;

    fn spawn(
        &self,
        slug: &Slug,
        role: &Role,
        cwd: &Path,
        argv: &[OsString],
    ) -> Result<RuntimeHandle, BleatError>;

    fn alive(&self, handle: &RuntimeHandle) -> Result<bool, BleatError>;

    fn nudge(&self, handle: &RuntimeHandle, slug: &Slug, role: &Role) -> Result<(), BleatError>;
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::path::Path;

    use super::{Runtime, RuntimeHandle};
    use crate::error::BleatError;
    use crate::identity::{Role, Slug};

    struct FakeRuntime {
        spawned_argv: RefCell<Vec<OsString>>,
    }

    impl Runtime for FakeRuntime {
        fn current_handle(&self) -> Result<RuntimeHandle, BleatError> {
            Ok(runtime_handle())
        }

        fn spawn(
            &self,
            _slug: &Slug,
            _role: &Role,
            _cwd: &Path,
            argv: &[OsString],
        ) -> Result<RuntimeHandle, BleatError> {
            self.spawned_argv.replace(argv.to_vec());
            Ok(runtime_handle())
        }

        fn alive(&self, _handle: &RuntimeHandle) -> Result<bool, BleatError> {
            Ok(true)
        }

        fn nudge(
            &self,
            _handle: &RuntimeHandle,
            _slug: &Slug,
            _role: &Role,
        ) -> Result<(), BleatError> {
            Ok(())
        }
    }

    #[test]
    fn spawn_receives_argv_as_os_string_sequence() {
        let runtime = FakeRuntime {
            spawned_argv: RefCell::new(Vec::new()),
        };
        let slug = Slug::parse("session").expect("fixture should be valid");
        let role = Role::parse("codex").expect("fixture should be valid");
        let argv = [
            OsString::from("codex"),
            OsString::from("argument with spaces"),
            OsString::from("$(echo not-a-shell)"),
        ];

        runtime
            .spawn(&slug, &role, Path::new("/project"), &argv)
            .expect("fake runtime should spawn");

        assert_eq!(runtime.spawned_argv.into_inner(), argv);
    }

    fn runtime_handle() -> RuntimeHandle {
        RuntimeHandle {
            terminal_id: "terminal-1".to_owned(),
            pane_id: "pane-1".to_owned(),
            agent_name: None,
        }
    }
}
