//! `openapi` and `version` through the real binary with an empty environment: no database, no master key and
//! no configuration at all (AZ-3811 AC-2, AC-3). The `healthcheck` probe is tested next to its code.

use std::process::{Command, Output};

use sunnycloud_http_api::current_document;

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
fn openapi_prints_the_same_bytes_every_run() {
    // Act
    let first = sunnycloud(&["openapi"]);
    let second = sunnycloud(&["openapi"]);

    // Assert
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty(), "{}", text(&first.stderr));
}

#[test]
fn openapi_is_valid_json_equal_to_the_document_the_server_serves() {
    // Act
    let output = sunnycloud(&["openapi"]);

    // Assert: 11 serves `current_document()` at `GET /api/v1/openapi.json` (its parity tests compare the two).
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(document["paths"].is_object());
    assert_eq!(output.stdout, current_document().as_bytes());
}

#[test]
fn openapi_with_the_json_flag_is_the_document_without_an_envelope() {
    // Act
    let plain = sunnycloud(&["openapi"]);
    let flagged = sunnycloud(&["--json", "openapi"]);

    // Assert
    assert_eq!(flagged.status.code(), Some(0), "{}", text(&flagged.stderr));
    assert_eq!(plain.stdout, flagged.stdout);
}

#[test]
fn version_prints_a_semver_and_the_git_commit_without_configuration() {
    // Act
    let output = sunnycloud(&["--json", "version"]);

    // Assert
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let object: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("one JSON object");
    let version = object["data"]["version"].as_str().expect("version");
    assert!(is_semver(version), "{version}");
    let commit = object["data"]["commit"].as_str().expect("commit");
    match option_env!("SUNNYCLOUD_GIT_COMMIT") {
        Some(stamped) => assert_eq!(commit, stamped),
        None => assert_eq!(commit, "unknown"),
    }
}

#[test]
fn the_build_stamps_a_commit_hash_when_git_or_the_variable_provides_one() {
    // Act
    let stamped = option_env!("SUNNYCLOUD_GIT_COMMIT");

    // Assert: a checkout (or an image build that passes the variable) never reports `unknown`.
    if let Some(commit) = stamped {
        assert!(
            commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()),
            "{commit}"
        );
    }
}

fn is_semver(text: &str) -> bool {
    let core = text.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
