use assert_cmd::cargo::cargo_bin_cmd;
#[test]
fn help_lists_only_pure_messaging_commands() {
    let output = cargo_bin_cmd!("bleat")
        .arg("--help")
        .output()
        .expect("bleat should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

    for expected in ["init", "join", "send", "read", "status", "log"] {
        assert!(
            stdout.contains(expected),
            "help should list `{expected}`; actual output:\n{stdout}"
        );
    }
}

#[test]
fn read_help_lists_wait_and_timeout() {
    let output = cargo_bin_cmd!("bleat")
        .args(["read", "--help"])
        .output()
        .expect("bleat should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
    assert!(stdout.contains("--wait"));
    assert!(stdout.contains("--timeout"));
}

#[test]
fn read_rejects_peek_with_wait() {
    let output = cargo_bin_cmd!("bleat")
        .args(["read", "--peek", "--wait"])
        .output()
        .expect("bleat should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");
    assert!(stderr.contains("cannot be used with"));
}

#[test]
fn read_rejects_a_negative_timeout() {
    let output = cargo_bin_cmd!("bleat")
        .args(["read", "--wait", "--timeout=-1"])
        .output()
        .expect("bleat should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");
    assert!(stderr.contains("invalid value"));
}

#[test]
fn read_rejects_timeout_without_wait() {
    let output = cargo_bin_cmd!("bleat")
        .args(["read", "--timeout", "1"])
        .output()
        .expect("bleat should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("error should be UTF-8");
    assert!(stderr.contains("required arguments were not provided"));
}
