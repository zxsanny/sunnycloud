//! AC-1 (IT-12-1): the exit and JSON rules, for every command of the contract.

use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use serde_json::Value;
use sunnycloud_server::{Class, CliError, Outcome};

use crate::common::words;
use crate::support::{COMMANDS, Harness, answer};

fn owner_refusal() -> CliError {
    DomainError::Business(BusinessError::new(
        ErrorCode::LastAdmin,
        BusinessParams::default(),
    ))
    .into()
}

fn one_object(stdout: &str) -> serde_json::Map<String, Value> {
    assert_eq!(stdout.lines().count(), 1, "one line: {stdout:?}");
    match serde_json::from_str::<Value>(stdout).expect("stdout is JSON") {
        Value::Object(object) => object,
        other => panic!("not an object: {other}"),
    }
}

#[test]
fn every_command_exits_0_on_success_and_prints_one_envelope_with_json() {
    for (line, route) in COMMANDS {
        let mut args = vec!["--json"];
        args.extend(words(line));

        let result = Harness::new().run(&args);

        assert_eq!(result.exit, 0, "{line}: {}", result.stderr);
        let object = one_object(&result.stdout);
        assert_eq!(object["result"], "ok", "{line}");
        assert_eq!(object["code"], "ok", "{line}");
        assert!(object["data"].is_object(), "{line}");
        assert_eq!(result.calls, vec![(*route).to_owned()], "{line}");
    }
}

#[test]
fn every_command_exits_2_on_an_owner_refusal_with_the_owners_code_in_the_envelope() {
    for (line, _) in COMMANDS {
        // `serve` installs the process's one log subscriber; the success test above runs it once.
        if *line == "serve" {
            continue;
        }
        let mut args = vec!["--json"];
        args.extend(words(line));

        let result = Harness::new()
            .responder(answer(|| Err(owner_refusal())))
            .run(&args);

        assert_eq!(result.exit, 2, "{line}: {}", result.stderr);
        let object = one_object(&result.stdout);
        assert_eq!(object["code"], "last_admin", "{line}");
        assert_eq!(object["result"], "failed", "{line}");
    }
}

#[test]
fn doctor_exits_0_for_ok_1_for_warnings_and_2_for_a_failure_and_json_names_the_level() {
    let cases = [
        (Class::Success, 0, "ok"),
        (Class::Warnings, 1, "warnings"),
        (Class::Findings, 2, "findings"),
    ];

    for (class, exit, name) in cases {
        let text = Harness::new()
            .responder(answer(move || Ok(Outcome::new(class))))
            .run(&["doctor"]);
        let json = Harness::new()
            .responder(answer(move || Ok(Outcome::new(class))))
            .run(&["--json", "doctor"]);

        assert_eq!(text.exit, exit, "{name}");
        assert_eq!(json.exit, exit, "{name}");
        assert_eq!(one_object(&json.stdout)["result"], name);
    }
}

#[test]
fn usage_errors_exit_3_with_no_business_call_and_nothing_on_stdout_even_with_json() {
    let bad: [&[&str]; 6] = [
        &["key", "rewrap"],
        &["admin", "reset"],
        &["admin", "user", "set-quota", "alice"],
        &["admin", "restore", "--account", "alice"],
        &["scrub"],
        &["nonsense"],
    ];

    for args in bad {
        for json in [false, true] {
            let mut full = if json { vec!["--json"] } else { vec![] };
            full.extend(args);

            let result = Harness::new().run(&full);

            assert_eq!(result.exit, 3, "{full:?}");
            assert!(result.calls.is_empty(), "{full:?}");
            assert_eq!(result.stdout, "", "{full:?}");
            assert!(!result.stderr.is_empty(), "{full:?}");
        }
    }
}

#[test]
fn the_four_destructive_commands_need_yes_on_a_non_tty_and_change_nothing_without_it() {
    let destructive: [&[&str]; 4] = [
        &["admin", "user", "delete", "alice"],
        &["admin", "rewrap", "--from-key-id", "1"],
        &["admin", "epoch-bump"],
        &["migrate", "revert"],
    ];

    for args in destructive {
        for json in [false, true] {
            let mut refused = if json { vec!["--json"] } else { vec![] };
            refused.extend(args);
            let mut confirmed = refused.clone();
            confirmed.push("--yes");

            let without = Harness::new().run(&refused);
            let with = Harness::new().run(&confirmed);

            assert_eq!(without.exit, 3, "{refused:?}");
            assert!(without.calls.is_empty(), "{refused:?}");
            assert_eq!(without.stdout, "", "{refused:?}");
            assert_eq!(with.exit, 0, "{confirmed:?}: {}", with.stderr);
            assert_eq!(with.calls.len(), 1, "{confirmed:?}");
        }
    }
}
