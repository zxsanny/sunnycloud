//! The real `sunnycloud` binary: exit codes and streams of the process itself.

use std::process::{Command, Output};

fn sunnycloud(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sunnycloud"));
    command.args(args);
    command.env_clear();
    command.output().expect("the binary starts")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

#[test]
fn version_prints_semver_and_succeeds_without_any_configuration() {
    let output = sunnycloud(&["version"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn json_version_is_one_object() {
    let output = sunnycloud(&["--json", "version"]);

    let object: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("one JSON object");
    assert_eq!(object["result"], "ok");
    assert_eq!(object["data"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn old_names_and_unknown_commands_exit_3() {
    for args in [
        &["key", "rewrap"][..],
        &["media", "requeue"],
        &["restore"],
        &["bogus"],
        &[],
    ] {
        let output = sunnycloud(args);

        assert_eq!(output.status.code(), Some(3), "{args:?}");
        assert!(text(&output.stderr).contains("Usage:"), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn a_destructive_command_without_yes_on_a_closed_stdin_exits_3() {
    let output = sunnycloud(&["admin", "user", "delete", "alice"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(text(&output.stderr).contains("--yes"));
}

#[test]
fn a_command_whose_task_has_not_landed_fails_loudly() {
    let output = sunnycloud(&["admin", "user", "list"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("not implemented"));
    assert!(output.stdout.is_empty());
}
