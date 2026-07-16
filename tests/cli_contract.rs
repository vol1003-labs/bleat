use std::fs;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use tempfile::tempdir;

#[test]
fn init_uses_the_explicit_role() {
    let project = tempdir().expect("project should be created");

    let output = bleat(project.path(), ["--as", "claude", "init", "feature"]);

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(project.path().join(".bleat/feature/session.json"))
            .expect("session should be readable"),
    )
    .expect("session should decode");
    assert_eq!(value["slug"], "feature");
    assert!(value["roles"].get("claude").is_some());
}

#[test]
fn init_uses_bleat_role_when_as_is_absent() {
    let project = tempdir().expect("project should be created");

    let output = command(project.path())
        .env("BLEAT_ROLE", "claude")
        .args(["init", "feature"])
        .output()
        .expect("bleat should run");

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(project.path().join(".bleat/feature/session.json"))
            .expect("session should be readable"),
    )
    .expect("session should decode");
    assert!(value["roles"].get("claude").is_some());
}

#[test]
fn join_registers_the_requested_role() {
    let project = initialized_project();

    let output = bleat(project.path(), ["--as", "codex", "join"]);

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(project.path().join(".bleat/feature/session.json"))
            .expect("session should be readable"),
    )
    .expect("session should decode");
    assert!(value["roles"].get("codex").is_some());
}

#[test]
fn send_then_read_delivers_and_consumes_a_message() {
    let project = joined_project();
    let sent = bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "hello"],
    );
    assert!(sent.status.success());

    let first = bleat(project.path(), ["--as", "codex", "read"]);
    let second = bleat(project.path(), ["--as", "codex", "read"]);

    assert!(first.status.success());
    assert!(
        String::from_utf8(first.stdout)
            .expect("read output should be UTF-8")
            .contains("hello")
    );
    assert!(second.status.success());
    assert!(second.stdout.is_empty());
}

#[test]
fn read_peek_does_not_consume_a_message() {
    let project = joined_project();
    let sent = bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "hello"],
    );
    assert!(sent.status.success());

    let peeked = bleat(project.path(), ["--as", "codex", "read", "--peek"]);
    let read = bleat(project.path(), ["--as", "codex", "read"]);

    assert!(peeked.status.success());
    assert!(read.status.success());
    assert!(
        String::from_utf8(read.stdout)
            .expect("read output should be UTF-8")
            .contains("hello")
    );
}

#[test]
fn log_displays_every_message() {
    let project = joined_project();
    bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "first"],
    );
    bleat(
        project.path(),
        ["--as", "codex", "send", "--to", "claude", "second"],
    );

    let output = bleat(project.path(), ["log"]);
    let stdout = String::from_utf8(output.stdout).expect("log output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("first"));
    assert!(stdout.contains("second"));
}

#[test]
fn status_displays_roles_counts_and_last_activity() {
    let project = joined_project();
    bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "hello"],
    );

    let output = bleat(project.path(), ["status"]);
    let stdout = String::from_utf8(output.stdout).expect("status output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Session: feature"));
    assert!(stdout.contains("- claude: 0 unread"));
    assert!(stdout.contains("- codex: 1 unread"));
    assert!(stdout.contains("Total messages: 1"));
    assert!(stdout.contains("claude -> codex"));
}

#[test]
fn missing_role_exits_with_code_2() {
    let project = tempdir().expect("project should be created");

    let output = bleat(project.path(), ["init", "feature"]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn missing_project_exits_with_code_2() {
    let project = tempdir().expect("project should be created");

    let output = bleat(project.path(), ["--as", "claude", "read"]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn read_wait_returns_existing_unread_immediately_with_code_0() {
    let project = joined_project();
    let sent = bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "waiting"],
    );
    assert!(sent.status.success());

    let output = bleat(project.path(), ["--as", "codex", "read", "--wait"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .expect("read output should be UTF-8")
            .contains("waiting")
    );
}

#[test]
fn read_wait_returns_after_another_process_sends_with_code_0() {
    let project = joined_project();
    let mut waiting = command(project.path());
    waiting
        .args(["--as", "codex", "read", "--wait", "--timeout", "2"])
        .env_remove("BLEAT_ROLE")
        .env_remove("BLEAT_SESSION")
        .env("BLEAT_POLL_INTERVAL_MS", "5")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut waiting = waiting.spawn().expect("waiting read should start");
    thread::sleep(Duration::from_millis(50));
    assert!(
        waiting
            .try_wait()
            .expect("waiting read status should be available")
            .is_none(),
        "read should still be waiting before send"
    );

    let sent = bleat(
        project.path(),
        ["--as", "claude", "send", "--to", "codex", "arrived"],
    );
    assert!(sent.status.success());
    let output = waiting
        .wait_with_output()
        .expect("waiting read should finish");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .expect("read output should be UTF-8")
            .contains("arrived")
    );
}

#[test]
fn read_wait_times_out_without_output_with_code_3() {
    let project = joined_project();

    let output = command(project.path())
        .args(["--as", "codex", "read", "--wait", "--timeout", "1"])
        .env_remove("BLEAT_ROLE")
        .env_remove("BLEAT_SESSION")
        .env("BLEAT_POLL_INTERVAL_MS", "5")
        .output()
        .expect("bleat should run");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn read_wait_rejects_timeout_zero_with_code_2() {
    let project = joined_project();

    let output = bleat(
        project.path(),
        ["--as", "codex", "read", "--wait", "--timeout", "0"],
    );

    assert_eq!(output.status.code(), Some(2));
}

fn initialized_project() -> tempfile::TempDir {
    let project = tempdir().expect("project should be created");
    let output = bleat(project.path(), ["--as", "claude", "init", "feature"]);
    assert!(output.status.success(), "init should succeed");
    project
}

fn joined_project() -> tempfile::TempDir {
    let project = initialized_project();
    let output = bleat(project.path(), ["--as", "codex", "join"]);
    assert!(output.status.success(), "join should succeed");
    project
}

fn bleat<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> Output {
    command(cwd)
        .args(args)
        .env_remove("BLEAT_ROLE")
        .env_remove("BLEAT_SESSION")
        .output()
        .expect("bleat should run")
}

fn command(cwd: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bleat"));
    command.current_dir(cwd);
    command
}
