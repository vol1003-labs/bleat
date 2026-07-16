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
