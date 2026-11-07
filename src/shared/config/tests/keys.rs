//! `load_keys`: a command binds only the keys it uses, so another command's missing or invalid setting never
//! refuses it, while unknown variables and the wanted keys still do.

mod common;

use common::*;
use sc_config::{
    ApiSettings, ConfigError, ConfigSources, KeyProblem, Reader, Reason, load, load_keys,
};

/// API sources whose secret folder is empty: every required secret is missing.
fn without_secrets() -> (ConfigSources, TempDir) {
    let mut sources = valid_sources(Reader::Api);
    let dir = TempDir::new();
    sources.secrets_dir = dir.path().to_path_buf();
    (sources, dir)
}

fn problem(key: &str, reason: Reason) -> KeyProblem {
    KeyProblem::new(key, reason)
}

#[test]
fn keys_outside_the_list_are_not_required() {
    // Arrange
    let (sources, _dir) = without_secrets();

    // Act
    let command = load_keys::<ApiSettings>(&sources, &["public_url"]);
    let everything = load::<ApiSettings>(&sources);

    // Assert
    assert!(command.is_ok(), "{command:?}");
    assert!(everything.is_err());
}

#[test]
fn an_invalid_value_of_an_unlisted_key_does_not_refuse() {
    // Arrange
    let (mut sources, _dir) = without_secrets();
    set_key(&mut sources, "retention", "91d");

    // Act
    let unlisted = load_keys::<ApiSettings>(&sources, &["public_url"]);
    let listed = load_keys::<ApiSettings>(&sources, &["public_url", "retention"]);

    // Assert
    assert!(unlisted.is_ok(), "{unlisted:?}");
    let Err(ConfigError::Invalid(problems)) = listed else {
        panic!("expected a refusal, got {listed:?}");
    };
    assert!(problems.iter().any(|p| p.key == "retention"));
}

#[test]
fn every_missing_listed_secret_is_reported_together() {
    // Arrange
    let (sources, _dir) = without_secrets();

    // Act
    let error = load_keys::<ApiSettings>(&sources, &["db_api_url", "master_key_file"])
        .expect_err("two secrets are missing");

    // Assert
    let ConfigError::Invalid(mut problems) = error else {
        panic!("expected one refusal, got {error:?}");
    };
    problems.sort_by(|a, b| a.key.cmp(&b.key));
    assert_eq!(
        problems,
        [
            problem("db_api_url", Reason::MissingRequired),
            problem("master_key_file", Reason::MissingRequired)
        ]
    );
}

#[test]
fn one_missing_listed_secret_alone_is_missing_secret() {
    // Arrange
    let (sources, _dir) = without_secrets();

    // Act
    let error = load_keys::<ApiSettings>(&sources, &["db_api_url"]).expect_err("missing");

    // Assert
    assert_eq!(error, ConfigError::MissingSecret("db_api_url".into()));
}

#[test]
fn an_unknown_variable_still_refuses_whatever_the_list() {
    // Arrange
    let (mut sources, _dir) = without_secrets();
    set(&mut sources, "SUNNYCLOUD_BOGUS", "1");

    // Act
    let error = load_keys::<ApiSettings>(&sources, &["public_url"]).expect_err("unknown");

    // Assert
    let ConfigError::Invalid(problems) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(problems, [problem("SUNNYCLOUD_BOGUS", Reason::Unknown)]);
}

#[test]
fn an_unreadable_secret_file_of_an_unlisted_key_does_not_refuse() {
    // Arrange
    let (mut sources, _dir) = without_secrets();
    set(
        &mut sources,
        "SUNNYCLOUD_DB_API_URL_FILE",
        "/nonexistent/db_api_url",
    );

    // Act
    let unlisted = load_keys::<ApiSettings>(&sources, &["public_url"]);
    let listed = load_keys::<ApiSettings>(&sources, &["db_api_url"]);

    // Assert
    assert!(unlisted.is_ok(), "{unlisted:?}");
    assert!(
        matches!(listed, Err(ConfigError::Unreadable(..))),
        "{listed:?}"
    );
}
