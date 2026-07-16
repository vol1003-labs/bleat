use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;
use std::process::{Command, Output};

use serde::{Deserialize, Serialize};

use super::{Runtime, RuntimeHandle};
use crate::error::BleatError;
use crate::identity::{Role, Slug};

trait ProcessRunner {
    fn run(&self, program: &OsStr, args: &[OsString]) -> io::Result<Output>;
}

impl ProcessRunner for fn(&OsStr, &[OsString]) -> io::Result<Output> {
    fn run(&self, program: &OsStr, args: &[OsString]) -> io::Result<Output> {
        self(program, args)
    }
}

pub struct HerdrRuntime<R = fn(&OsStr, &[OsString]) -> io::Result<Output>> {
    runner: R,
    env_pane_id: Option<OsString>,
}

impl HerdrRuntime<fn(&OsStr, &[OsString]) -> io::Result<Output>> {
    pub fn new() -> Self {
        Self {
            runner: run_command,
            env_pane_id: env::var_os("HERDR_PANE_ID"),
        }
    }
}

impl Default for HerdrRuntime<fn(&OsStr, &[OsString]) -> io::Result<Output>> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> HerdrRuntime<R> {
    #[cfg(test)]
    fn with_runner(runner: R, env_pane_id: Option<OsString>) -> Self {
        Self {
            runner,
            env_pane_id,
        }
    }
}

fn run_command(program: &OsStr, args: &[OsString]) -> io::Result<Output> {
    Command::new(program).args(args).output()
}

#[derive(Deserialize)]
struct PaneEnvelope {
    result: PaneResult,
}

#[derive(Deserialize)]
struct PaneResult {
    pane: Pane,
}

#[derive(Deserialize)]
struct Pane {
    pane_id: String,
    terminal_id: String,
}

fn pane<R: ProcessRunner>(
    runtime: &HerdrRuntime<R>,
    args: Vec<OsString>,
) -> Result<Pane, BleatError> {
    let output = runtime
        .runner
        .run(OsStr::new("herdr"), &args)
        .map_err(|error| BleatError::Runtime(format!("failed to run herdr: {error}")))?;

    if !output.status.success() {
        return Err(BleatError::Runtime(format!(
            "herdr exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    serde_json::from_slice::<PaneEnvelope>(&output.stdout)
        .map(|envelope| envelope.result.pane)
        .map_err(|error| BleatError::Runtime(format!("invalid herdr JSON response: {error}")))
}

fn pane_by_id<R: ProcessRunner>(
    runtime: &HerdrRuntime<R>,
    pane_id: &OsStr,
) -> Result<Pane, BleatError> {
    pane(
        runtime,
        vec![
            OsString::from("pane"),
            OsString::from("get"),
            pane_id.to_os_string(),
        ],
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HerdrHandle {
    terminal_id: String,
    pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_name: Option<String>,
}

impl RuntimeHandle for HerdrHandle {}

impl<R: ProcessRunner> Runtime for HerdrRuntime<R> {
    type Handle = HerdrHandle;

    fn current_handle(&self) -> Result<Self::Handle, BleatError> {
        let pane = match self.env_pane_id.as_deref() {
            Some(pane_id) => pane_by_id(self, pane_id)?,
            None => pane(
                self,
                vec![OsString::from("pane"), OsString::from("current")],
            )?,
        };

        Ok(HerdrHandle {
            terminal_id: pane.terminal_id,
            pane_id: pane.pane_id,
            agent_name: None,
        })
    }

    fn spawn(
        &self,
        _slug: &Slug,
        _role: &Role,
        _cwd: &Path,
        _argv: &[OsString],
    ) -> Result<Self::Handle, BleatError> {
        Err(BleatError::Runtime("herdr spawn is not implemented".into()))
    }

    fn alive(&self, _handle: &Self::Handle) -> Result<bool, BleatError> {
        Err(BleatError::Runtime(
            "herdr alive check is not implemented".into(),
        ))
    }

    fn nudge(&self, _handle: &Self::Handle, _slug: &Slug, _role: &Role) -> Result<(), BleatError> {
        Err(BleatError::Runtime("herdr nudge is not implemented".into()))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use super::{HerdrRuntime, ProcessRunner};
    use crate::error::BleatError;
    use crate::runtime::Runtime;

    #[derive(Debug, Eq, PartialEq)]
    struct Invocation {
        program: OsString,
        args: Vec<OsString>,
    }

    struct RecordingRunner {
        outputs: RefCell<VecDeque<io::Result<Output>>>,
        invocations: RefCell<Vec<Invocation>>,
    }

    impl RecordingRunner {
        fn new(outputs: impl IntoIterator<Item = io::Result<Output>>) -> Self {
            Self {
                outputs: RefCell::new(outputs.into_iter().collect()),
                invocations: RefCell::new(Vec::new()),
            }
        }
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, program: &OsStr, args: &[OsString]) -> io::Result<Output> {
            self.invocations.borrow_mut().push(Invocation {
                program: program.to_os_string(),
                args: args.to_vec(),
            });

            self.outputs.borrow_mut().pop_front().unwrap_or_else(|| {
                Err(io::Error::other(
                    "recording runner received an unexpected invocation",
                ))
            })
        }
    }

    fn success(stdout: &str) -> io::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    fn failure(stderr: &str) -> io::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    fn pane_response(kind: &str, pane_id: &str, terminal_id: Option<&str>) -> String {
        let terminal = terminal_id
            .map(|id| format!(r#", "terminal_id": "{id}""#))
            .unwrap_or_default();
        format!(
            r#"{{"id":"fixture","result":{{"pane":{{"pane_id":"{pane_id}"{terminal},"future_field":7}},"type":"{kind}"}}}}"#
        )
    }

    #[test]
    fn current_handle_prefers_env_pane_id() {
        let env_pane_id = "pane with spaces; $(echo nope)";
        let runner = RecordingRunner::new([success(&pane_response(
            "pane_info",
            env_pane_id,
            Some("term-env"),
        ))]);
        let runtime = HerdrRuntime::with_runner(runner, Some(OsString::from(env_pane_id)));

        let handle = runtime.current_handle().unwrap();

        assert_eq!(
            serde_json::to_value(&handle).expect("handle should serialize"),
            serde_json::json!({
                "terminal_id": "term-env",
                "pane_id": env_pane_id
            })
        );
        assert_eq!(
            runtime.runner.invocations.into_inner(),
            vec![Invocation {
                program: OsString::from("herdr"),
                args: ["pane", "get", env_pane_id]
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
            }]
        );
    }

    #[test]
    fn current_handle_falls_back_to_current_pane() {
        let runner = RecordingRunner::new([success(&pane_response(
            "pane_current",
            "w1:p7",
            Some("term-current"),
        ))]);
        let runtime = HerdrRuntime::with_runner(runner, None);

        let handle = runtime.current_handle().unwrap();

        assert_eq!(
            serde_json::to_value(&handle).expect("handle should serialize"),
            serde_json::json!({
                "terminal_id": "term-current",
                "pane_id": "w1:p7"
            })
        );
        assert_eq!(
            runtime.runner.invocations.into_inner(),
            vec![Invocation {
                program: OsString::from("herdr"),
                args: ["pane", "current"]
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
            }]
        );
    }

    #[test]
    fn current_handle_rejects_invalid_json() {
        let runner = RecordingRunner::new([success("not json")]);
        let runtime = HerdrRuntime::with_runner(runner, None);

        let error = runtime.current_handle().unwrap_err();

        assert!(matches!(error, BleatError::Runtime(message) if message.contains("JSON")));
    }

    #[test]
    fn current_handle_includes_stderr_for_nonzero_status() {
        let runner = RecordingRunner::new([failure("pane lookup failed")]);
        let runtime = HerdrRuntime::with_runner(runner, None);

        let error = runtime.current_handle().unwrap_err();

        assert!(
            matches!(error, BleatError::Runtime(message) if message.contains("pane lookup failed"))
        );
    }

    #[test]
    fn current_handle_rejects_missing_terminal_id() {
        let runner = RecordingRunner::new([success(&pane_response("pane_info", "w1:p9", None))]);
        let runtime = HerdrRuntime::with_runner(runner, Some(OsString::from("w1:p9")));

        let error = runtime.current_handle().unwrap_err();

        assert!(matches!(error, BleatError::Runtime(message) if message.contains("terminal_id")));
    }
}
