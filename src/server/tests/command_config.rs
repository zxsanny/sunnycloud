//! AC-1, AC-3 and AC-5 of AZ-3790 at the library level: what each command asks of configuration, the single
//! refusal of `serve`, and the text of a refusal.

mod config_support;

use std::collections::BTreeSet;
use std::path::Path;

use clap::Parser;
use config_support::{PLANTED, Setup};
use sc_config::{ApiSettings, ConfigError, ConfigSources, load_keys};
use sunnycloud_server::commands::Cli;
use sunnycloud_server::{ConfigCheck, KeyGroup, ServeSettings};

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
fn each_command_asks_only_for_the_keys_of_what_it_builds() {
    // Arrange
    let keyring = ConfigCheck::Services(&[KeyGroup::Database, KeyGroup::Blobstore]);
    let invites = ConfigCheck::Services(&[KeyGroup::Database, KeyGroup::Identity]);

    // Act
    let unrestricted = [
        &["version"][..],
        &["openapi"],
        &["healthcheck"],
        &["admin", "promote", "00000000-0000-0000-0000-000000000001"],
    ]
    .map(check_of);
    let keyring_commands = [
        &["key-check"][..],
        &["admin", "keycheck-init"],
        &["admin", "rewrap", "--from-key-id", "1", "--yes"],
    ]
    .map(check_of);
    let invite_commands = [
        &["admin", "setup-token"][..],
        &["admin", "create"],
        &["admin", "reset", "00000000-0000-0000-0000-000000000001"],
        &[
            "admin",
            "invite",
            "--admin",
            "00000000-0000-0000-0000-000000000001",
            "list",
        ],
    ]
    .map(check_of);

    // Assert
    assert!(unrestricted.iter().all(|c| *c == ConfigCheck::Unrestricted));
    assert!(
        keyring_commands.iter().all(|c| *c == keyring),
        "{keyring_commands:?}"
    );
    assert!(
        invite_commands.iter().all(|c| *c == invites),
        "{invite_commands:?}"
    );
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

/// The table keys an owner's configuration source reads: every `settings.<key>` in the file.
fn keys_read_by(source: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(source);
    let text = std::fs::read_to_string(&path).expect("the owner's configuration source");
    let mut keys = BTreeSet::new();
    for (start, _) in text.match_indices("settings.") {
        let key: String = text[start + "settings.".len()..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        if !key.is_empty() {
            keys.insert(key);
        }
    }
    keys
}

#[test]
fn each_key_group_lists_exactly_the_keys_its_owner_reads() {
    // Arrange: the migrator secret is helper 07's, but the API role never connects with it.
    let cases = [
        (
            KeyGroup::Database,
            "../shared/db/src/config.rs",
            &["db_migrator_url"][..],
        ),
        (
            KeyGroup::Blobstore,
            "../components/04_blobstore/src/config.rs",
            &[],
        ),
        (
            KeyGroup::Identity,
            "../components/01_identity/src/config.rs",
            &[],
        ),
    ];

    for (group, source, not_bound) in cases {
        // Act
        let mut read = keys_read_by(source);
        for key in not_bound {
            read.remove(*key);
        }
        let listed: BTreeSet<String> = group.keys().iter().map(|key| (*key).to_owned()).collect();

        // Assert
        assert_eq!(listed, read, "{group:?} against {source}");
    }
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
