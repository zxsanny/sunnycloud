//! AC-9: one structured log line per command, with no secret and no user path.

mod support;

use serde_json::Value;
use sunnycloud_server::{Class, CliError, Outcome};
use support::{Harness, answer};

#[test]
fn one_line_per_command_without_the_argument_values() {
    let result = Harness::new().log_json().run(&[
        "admin",
        "rewrap",
        "--new-key-file",
        "/home/alice/secret-master.key",
        "--yes",
    ]);

    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert_eq!(result.log.len(), 1, "{:?}", result.log);
    let line: Value = serde_json::from_str(&result.log[0]).expect("a JSON line");
    assert_eq!(line["route"], "/cli/admin-rewrap");
    assert_eq!(line["msg"], "cli command finished: exit 0");
    assert!(line["duration_ms"].is_u64());
    for text in [&result.log[0], &result.stdout, &result.stderr] {
        assert!(
            !text.contains("secret-master") && !text.contains("/home/alice"),
            "{text}"
        );
    }
}

#[test]
fn the_line_carries_the_exit_code_and_the_failure() {
    let warn = Harness::new()
        .log_json()
        .responder(answer(|| Ok(Outcome::new(Class::Warnings))))
        .run(&["doctor"]);
    let fail = Harness::new()
        .log_json()
        .responder(answer(|| Err(CliError::NotImplemented("/cli/doctor"))))
        .run(&["doctor"]);
    let usage = Harness::new()
        .log_json()
        .responder(answer(|| Err(CliError::Usage("bad".into()))))
        .run(&["doctor"]);

    assert!(warn.log[0].contains("exit 1") && warn.log[0].contains("\"level\":\"warn\""));
    assert!(fail.log[0].contains("exit 2") && fail.log[0].contains("\"level\":\"error\""));
    assert!(fail.log[0].contains("not implemented"));
    assert!(usage.log[0].contains("exit 3"));
}

#[test]
fn text_logging_and_usage_errors_write_no_command_line() {
    let text = Harness::new().run(&["doctor"]);
    let usage = Harness::new().log_json().run(&["nonsense"]);

    assert!(text.log.is_empty(), "{:?}", text.log);
    assert!(usage.log.is_empty(), "{:?}", usage.log);
}
