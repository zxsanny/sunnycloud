//! AC-1, AC-3 and AC-5 of AZ-3790 at the library level: what each command asks of configuration, the single
//! refusal of `serve`, and the text of a refusal.

mod config_support;

use clap::Parser;
use config_support::{PLANTED, Setup};
use sc_config::{ApiSettings, ConfigError, ConfigSources, load_keys};
use sunnycloud_server::commands::Cli;
use sunnycloud_server::{ConfigCheck, ServeSettings};

fn check_of(args: &[&str]) -> ConfigCheck {
    let argv = std::iter::once("sunnycloud").chain(args.iter().copied());
    Cli::try_parse_from(argv)
        .expect("the arguments parse")
        .command
        .config_check()
}

fn sources(setup: &Setup) -> ConfigSources {
    ConfigSources {
        env: setup.env.clone(),
        ..ConfigSources::default()
    }
}

fn refusal(setup: &Setup) -> ConfigError {
    match load_keys::<ServeSettings>(&sources(setup), &ConfigCheck::Server.keys()) {
        Err(error) => error,
        Ok(_) => panic!("the configuration should be refused"),
    }
}

fn problem_keys(error: &ConfigError) -> Vec<String> {
    match error {
        ConfigError::Invalid(problems) => problems.iter().map(|p| p.key.clone()).collect(),
        other => panic!("expected a list of problems, got {other:?}"),
    }
}

#[test]
fn only_serve_and_the_migrate_commands_ask_anything_of_configuration() {
    // Act
    let unrestricted = [
        &["version"][..],
        &["openapi"],
        &["healthcheck"],
        &["key-check"],
        &["admin", "setup-token"],
    ]
    .map(check_of);

    // Assert
    assert!(unrestricted.iter().all(|c| *c == ConfigCheck::Unrestricted));
    assert_eq!(check_of(&["migrate"]), ConfigCheck::MigratorUrl);
    assert_eq!(check_of(&["migrate", "run"]), ConfigCheck::MigratorUrl);
    assert_eq!(
        check_of(&["migrate", "revert", "--yes"]),
        ConfigCheck::MigratorUrl
    );
    assert_eq!(
        check_of(&["migrate", "status"]),
        ConfigCheck::AnyDatabaseUrl
    );
    assert_eq!(check_of(&["serve"]), ConfigCheck::Server);
}

#[test]
fn a_complete_configuration_passes_serve_validation() {
    // Arrange
    let setup = Setup::valid(1);

    // Act
    let loaded = load_keys::<ServeSettings>(&sources(&setup), &ConfigCheck::Server.keys());

    // Assert
    assert!(loaded.is_ok(), "{loaded:?}");
}

#[test]
fn the_migrator_step_binds_no_key_of_another_section() {
    // Arrange: no provider, no worker token, no master key, no API database URL, a bad retention.
    let mut setup = Setup::valid(1);
    setup.remove("github_client_id");
    setup.remove("public_url");
    for secret in [
        "provider_github_client_secret",
        "worker_api_token",
        "master_key_file",
        "share_grant_key",
        "db_api_url",
    ] {
        setup.without_secret(secret);
    }
    setup.set("retention", "91d");
    let keys = ConfigCheck::MigratorUrl.keys();

    // Act
    let migrate = load_keys::<ApiSettings>(&sources(&setup), &keys);
    let serve = refusal(&setup);

    // Assert
    assert!(migrate.is_ok(), "{migrate:?}");
    assert!(keys.contains(&"db_migrator_url") && keys.contains(&"db_pool_max"));
    assert!(!keys.contains(&"db_api_url") && !keys.contains(&"master_key_file"));
    assert!(problem_keys(&serve).len() >= 6, "{serve:?}");
}

#[test]
fn migrate_status_binds_the_database_keys_of_either_role_and_nothing_else() {
    // Act
    let keys = ConfigCheck::AnyDatabaseUrl.keys();

    // Assert
    assert!(keys.contains(&"db_migrator_url") && keys.contains(&"db_api_url"));
    assert!(
        keys.iter()
            .all(|key| key.starts_with("db_") || *key == "environment")
    );
}

#[test]
fn serve_binds_every_server_secret_so_a_refusal_lists_each_missing_one() {
    // Arrange
    let mut setup = Setup::valid(1);
    for secret in [
        "db_api_url",
        "master_key_file",
        "share_grant_key",
        "worker_api_token",
    ] {
        setup.without_secret(secret);
    }

    // Act
    let keys = problem_keys(&refusal(&setup));

    // Assert
    for secret in [
        "db_api_url",
        "master_key_file",
        "share_grant_key",
        "worker_api_token",
    ] {
        assert!(
            keys.contains(&secret.to_owned()),
            "{secret} missing from {keys:?}"
        );
    }
}

#[test]
fn a_provider_is_required_by_serve_only() {
    // Arrange
    let mut setup = Setup::valid(1);
    setup.remove("github_client_id");
    setup.without_secret("provider_github_client_secret");

    // Act
    let migrate = load_keys::<ApiSettings>(&sources(&setup), &ConfigCheck::MigratorUrl.keys());
    let serve = refusal(&setup);

    // Assert
    assert!(migrate.is_ok(), "{migrate:?}");
    assert!(problem_keys(&serve).contains(&"providers".to_owned()));
}

#[test]
fn three_invalid_keys_in_three_sections_are_one_refusal() {
    // Arrange: identity and blobstore are caught by the loader's ranges, sharing by its owner's validation.
    let mut setup = Setup::valid(1);
    setup.set("refresh_reuse_grace", "5m");
    setup.set("retention", "91d");
    setup.set("share_wrong_password_limit", "0");

    // Act
    let keys = problem_keys(&refusal(&setup));

    // Assert
    for key in [
        "refresh_reuse_grace",
        "retention",
        "share_wrong_password_limit",
    ] {
        assert!(
            keys.contains(&key.to_owned()),
            "{key} missing from {keys:?}"
        );
    }
    assert_eq!(keys.len(), 3, "{keys:?}");
}

#[test]
fn an_unknown_variable_and_an_out_of_range_retention_are_named_with_their_range() {
    // Arrange
    let mut setup = Setup::valid(1);
    setup.set("bogus", "1");
    setup.set("retention", "91d");

    // Act
    let text = sunnycloud_server::CliError::from(refusal(&setup)).to_string();

    // Assert
    assert!(text.contains("SUNNYCLOUD_BOGUS"), "{text}");
    assert!(
        text.contains("retention") && text.contains("0s to 90d"),
        "{text}"
    );
}

#[test]
fn a_secret_key_that_is_refused_shows_its_value_as_stars() {
    // Arrange
    let mut setup = Setup::valid(1);
    let short_key = "short-grant-key-value";
    setup.secret("share_grant_key", short_key);
    setup.set("retention", "91d");

    // Act
    let text = sunnycloud_server::CliError::from(refusal(&setup)).to_string();

    // Assert
    assert!(text.contains("share_grant_key"), "{text}");
    assert!(text.contains("(value ***)"), "{text}");
    assert!(
        !text.contains(short_key) && !text.contains(PLANTED),
        "{text}"
    );
}

#[test]
fn a_missing_secret_is_named_and_the_planted_value_never_appears() {
    // Arrange
    let error = ConfigError::MissingSecret("db_migrator_url".to_owned());

    // Act
    let text = sunnycloud_server::CliError::from(error).to_string();

    // Assert
    assert!(text.contains("db_migrator_url"), "{text}");
    assert!(!text.contains("***"), "{text}");
}
