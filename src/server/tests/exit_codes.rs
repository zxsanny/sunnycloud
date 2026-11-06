//! AC-3 and AC-4: one exit-code mapping and one JSON object.

mod support;

use sc_config::ConfigError;
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};
use serde_json::Value;
use sunnycloud_server::{Class, CliError, Exit, Outcome};
use support::{Harness, answer};

fn last_admin() -> CliError {
    DomainError::Business(BusinessError::new(
        ErrorCode::LastAdmin,
        BusinessParams::default(),
    ))
    .into()
}

fn unexpected() -> CliError {
    DomainError::unexpected(
        std::io::Error::other("database exploded"),
        CorrelationId::parse("0198d5a1-1111-7222-8333-444455556666").expect("valid id"),
    )
    .into()
}

fn exit_of(make: impl Fn() -> Result<Outcome, CliError> + Send + Sync + 'static) -> u8 {
    Harness::new().responder(answer(make)).run(&["doctor"]).exit
}

#[test]
fn outcome_classes_and_failures_map_to_the_documented_codes() {
    assert_eq!(exit_of(|| Ok(Outcome::success())), 0);
    assert_eq!(exit_of(|| Ok(Outcome::new(Class::Warnings))), 1);
    assert_eq!(exit_of(|| Ok(Outcome::new(Class::Findings))), 2);
    assert_eq!(exit_of(|| Err(last_admin())), 2);
    assert_eq!(exit_of(|| Err(unexpected())), 2);
    assert_eq!(
        exit_of(|| Err(ConfigError::MissingSecret("db_migrator_url".into()).into())),
        3
    );
    assert_eq!(exit_of(|| Err(CliError::Usage("bad".into()))), 3);
    assert_eq!(
        exit_of(|| Err(CliError::Operator {
            code: "epoch_bump_failed",
            detail: "x".into(),
            exit: Exit::Warnings
        })),
        1
    );
}

#[test]
fn an_unfinished_command_fails_instead_of_pretending() {
    let result = Harness::new().real_handler().run(&["serve"]);

    assert_eq!(result.exit, 2);
    assert!(
        result.stderr.contains("not implemented"),
        "{}",
        result.stderr
    );
    assert_eq!(result.stdout, "");
}

fn parse_one_object(stdout: &str) -> serde_json::Map<String, Value> {
    assert_eq!(stdout.lines().count(), 1, "one line: {stdout:?}");
    assert!(stdout.ends_with('\n'));
    match serde_json::from_str::<Value>(stdout).expect("stdout is JSON") {
        Value::Object(object) => object,
        other => panic!("not an object: {other}"),
    }
}

#[test]
fn json_success_is_one_object_with_three_keys() {
    let result = Harness::new()
        .responder(answer(|| {
            Ok(Outcome::success().field("applied", 3).field("note", "ok"))
        }))
        .run(&["--json", "migrate"]);

    let object = parse_one_object(&result.stdout);
    let mut keys: Vec<_> = object.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["code", "data", "result"]);
    assert_eq!(object["result"], "ok");
    assert_eq!(object["code"], "ok");
    assert_eq!(object["data"]["applied"], 3);
    assert_eq!(result.exit, 0);
    assert_eq!(result.stderr, "");
}

#[test]
fn json_flag_may_follow_the_subcommand() {
    let result = Harness::new().run(&["doctor", "--json"]);

    assert_eq!(parse_one_object(&result.stdout)["result"], "ok");
}

#[test]
fn json_refusal_carries_the_owner_code_and_diagnostics_stay_on_stderr() {
    let result = Harness::new()
        .responder(answer(|| Err(last_admin())))
        .run(&["--json", "admin", "demote", "alice"]);

    let object = parse_one_object(&result.stdout);
    assert_eq!(object["code"], "last_admin");
    assert_eq!(object["result"], "failed");
    assert!(object["data"].is_object());
    assert_eq!(result.exit, 2);
    assert!(result.stderr.contains("last_admin"), "{}", result.stderr);
}

#[test]
fn text_mode_prints_the_same_facts_and_a_refusal_prints_nothing_on_stdout() {
    let facts = Harness::new()
        .responder(answer(|| {
            Ok(Outcome::success().field("applied", 3).field("note", "ok"))
        }))
        .run(&["migrate"]);
    let refusal = Harness::new()
        .responder(answer(|| Err(last_admin())))
        .run(&["admin", "demote", "alice"]);

    assert_eq!(facts.stdout, "applied: 3\nnote: ok\n");
    assert_eq!(refusal.stdout, "");
    assert_eq!(refusal.stderr, "error: last_admin\n");
}

#[test]
fn an_unexpected_failure_shows_the_opaque_message_not_its_source() {
    let result = Harness::new()
        .responder(answer(|| Err(unexpected())))
        .run(&["--json", "doctor"]);

    let object = parse_one_object(&result.stdout);
    assert_eq!(object["code"], "internal");
    assert!(!result.stdout.contains("exploded") && !result.stderr.contains("exploded"));
}

#[test]
fn usage_and_configuration_errors_leave_stdout_empty_even_with_json() {
    let usage = Harness::new().run(&["--json", "nonsense"]);
    let config = Harness::new()
        .responder(answer(|| {
            Err(ConfigError::MissingSecret("db_migrator_url".into()).into())
        }))
        .run(&["--json", "migrate"]);

    assert_eq!((usage.exit, usage.stdout.as_str()), (3, ""));
    assert_eq!((config.exit, config.stdout.as_str()), (3, ""));
    assert!(config.stderr.contains("db_migrator_url"));
}

#[test]
fn findings_are_a_result_not_an_error() {
    let result = Harness::new()
        .responder(answer(|| {
            Ok(Outcome::new(Class::Findings).field("mismatches", 2))
        }))
        .run(&["--json", "scrub", "run"]);

    let object = parse_one_object(&result.stdout);
    assert_eq!(
        (object["result"].as_str(), result.exit),
        (Some("findings"), 2)
    );
}
