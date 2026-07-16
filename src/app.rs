use std::env;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::time::Duration;

use chrono::Utc;

use crate::cli::{Cli, Command};
use crate::commands;
use crate::error::BleatError;
use crate::identity::{MessageType, Role, Slug};
use crate::root::find_project_root;
use crate::session::{Session, SessionPath, SessionResolution, load_session, resolve_session};
use crate::store::file::FileStore;

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u8,
}

impl CommandOutput {
    fn success(stdout: String, stderr: String) -> Self {
        Self {
            stdout: line_terminated(stdout),
            stderr,
            exit_code: 0,
        }
    }

    fn timeout() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 3,
        }
    }
}

pub fn run(cli: Cli) -> Result<CommandOutput, BleatError> {
    let Cli {
        session: session_flag,
        role: role_flag,
        command,
    } = cli;
    let cwd = env::current_dir().map_err(|source| {
        BleatError::Execution(format!("failed to read current directory: {source}"))
    })?;

    match command {
        Command::Init { slug } => {
            let role = resolve_role(role_flag.as_deref())?;
            commands::init::init(&cwd, Slug::parse(&slug)?, role, Utc::now().fixed_offset())?;
            Ok(CommandOutput::success(String::new(), String::new()))
        }
        Command::Join => {
            let (resolution, warning) = resolve_existing_session(session_flag.as_deref(), &cwd)?;
            let role = resolve_role(role_flag.as_deref())?;
            commands::join::join(&resolution.path, role, Utc::now().fixed_offset())?;
            Ok(CommandOutput::success(String::new(), warning))
        }
        Command::Send {
            to,
            kind,
            reply_to,
            file,
            body,
        } => {
            let (resolution, warning) = resolve_existing_session(session_flag.as_deref(), &cwd)?;
            let role = resolve_role(role_flag.as_deref())?;
            let session = load_session(&resolution.path.session_json())?;
            let store = file_store(&session, &resolution.path)?;
            let stdin = io::stdin();
            let stdin_is_tty = stdin.is_terminal();
            let mut stdin = stdin.lock();
            let body = commands::send::read_body(body, file.as_deref(), &mut stdin, stdin_is_tty)?;
            commands::send::send(
                &session,
                role,
                Role::parse(&to)?,
                kind.as_deref().map(MessageType::parse).transpose()?,
                reply_to,
                body,
                &store,
            )?;
            Ok(CommandOutput::success(String::new(), warning))
        }
        Command::Read {
            peek,
            wait,
            timeout,
        } => {
            let (resolution, warning) = resolve_existing_session(session_flag.as_deref(), &cwd)?;
            let role = resolve_role(role_flag.as_deref())?;
            let session = load_session(&resolution.path.session_json())?;
            let store = file_store(&session, &resolution.path)?;
            if wait {
                return match commands::read::wait(
                    &store,
                    &role,
                    wait_timeout(timeout)?,
                    poll_interval()?,
                )? {
                    Some(output) => Ok(CommandOutput::success(output, warning)),
                    None => Ok(CommandOutput::timeout()),
                };
            }
            let output = commands::read::read(&store, &role, peek)?;
            Ok(CommandOutput::success(output, warning))
        }
        Command::Status => {
            let (resolution, warning) = resolve_existing_session(session_flag.as_deref(), &cwd)?;
            let session = load_session(&resolution.path.session_json())?;
            let store = file_store(&session, &resolution.path)?;
            let output = commands::status::status(&session, &store)?;
            Ok(CommandOutput::success(output, warning))
        }
        Command::Log => {
            let (resolution, warning) = resolve_existing_session(session_flag.as_deref(), &cwd)?;
            let session = load_session(&resolution.path.session_json())?;
            let store = file_store(&session, &resolution.path)?;
            let output = commands::log::log(&store)?;
            Ok(CommandOutput::success(output, warning))
        }
    }
}

fn resolve_role(flag: Option<&str>) -> Result<Role, BleatError> {
    match flag {
        Some(role) => Role::parse(role),
        None => match env::var("BLEAT_ROLE") {
            Ok(role) => Role::parse(&role),
            Err(env::VarError::NotPresent) => Err(BleatError::Usage(
                "role is required via --as or BLEAT_ROLE".to_owned(),
            )),
            Err(env::VarError::NotUnicode(_)) => Err(BleatError::Usage(
                "BLEAT_ROLE must be valid UTF-8".to_owned(),
            )),
        },
    }
}

fn resolve_existing_session(
    session_flag: Option<&str>,
    cwd: &Path,
) -> Result<(SessionResolution, String), BleatError> {
    let root = find_project_root(cwd)?;
    let environment = match env::var("BLEAT_SESSION") {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(BleatError::Usage(
                "BLEAT_SESSION must be valid UTF-8".to_owned(),
            ));
        }
    };
    let resolution = resolve_session(&root, session_flag, environment.as_deref())?;
    let warning = resolution
        .warning
        .as_deref()
        .map(|warning| format!("{warning}\n"))
        .unwrap_or_default();
    Ok((resolution, warning))
}

fn file_store(session: &Session, path: &SessionPath) -> Result<FileStore, BleatError> {
    if session.store != "file" {
        return Err(BleatError::Execution(format!(
            "unsupported store `{}`",
            session.store
        )));
    }
    Ok(FileStore::new(path.directory()))
}

fn wait_timeout(timeout: Option<u64>) -> Result<Duration, BleatError> {
    let seconds = timeout.unwrap_or(300);
    if seconds == 0 {
        return Err(BleatError::Usage(
            "timeout must be greater than zero".to_owned(),
        ));
    }
    Ok(Duration::from_secs(seconds))
}

fn poll_interval() -> Result<Duration, BleatError> {
    match env::var("BLEAT_POLL_INTERVAL_MS") {
        Ok(value) => {
            let milliseconds = value.parse::<u64>().map_err(|_| {
                BleatError::Usage("BLEAT_POLL_INTERVAL_MS must be a positive integer".to_owned())
            })?;
            if milliseconds == 0 {
                return Err(BleatError::Usage(
                    "BLEAT_POLL_INTERVAL_MS must be a positive integer".to_owned(),
                ));
            }
            Ok(Duration::from_millis(milliseconds))
        }
        Err(env::VarError::NotPresent) => Ok(Duration::from_secs(2)),
        Err(env::VarError::NotUnicode(_)) => Err(BleatError::Usage(
            "BLEAT_POLL_INTERVAL_MS must be valid UTF-8".to_owned(),
        )),
    }
}

fn line_terminated(mut output: String) -> String {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_timeout_defaults_to_300_seconds() {
        assert_eq!(
            wait_timeout(None).expect("default timeout should be valid"),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn wait_timeout_uses_the_requested_seconds() {
        assert_eq!(
            wait_timeout(Some(7)).expect("positive timeout should be valid"),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn wait_timeout_rejects_zero() {
        let error = wait_timeout(Some(0)).expect_err("zero timeout should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }
}
