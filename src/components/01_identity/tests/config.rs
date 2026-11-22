//! AC-4 (defaults) and AC-5 (start validation) of the identity section. Defaults are read, never waited for.

mod support;

use std::time::Duration;

use sc_config::{KeyProblem, Reason};
use sunnycloud_identity::{IdentityConfig, IdentityError, IdentityProvidersCheck};
use support::apple::{APPLE, KEY_ID, SERVICES_ID, TEAM_ID, TeamKey};
use support::{identity_config, load_settings};

const MINUTE: u64 = 60;
const DAY: u64 = 86_400;

fn defaults() -> IdentityConfig {
    IdentityConfig::from_settings(&load_settings(&[]))
}

fn valid() -> IdentityConfig {
    identity_config(&load_settings(&[]))
}

fn keys(problems: &[KeyProblem]) -> Vec<&str> {
    problems
        .iter()
        .map(|problem| problem.key.as_str())
        .collect()
}

/// Refused with the key named, and accepted once `fix` is applied.
fn assert_refused_then_accepted(
    key: &str,
    break_it: impl FnOnce(&mut IdentityConfig),
    fix: impl FnOnce(&mut IdentityConfig),
) {
    refused_then_accepted(valid(), key, break_it, fix);
}

fn refused_then_accepted(
    mut config: IdentityConfig,
    key: &str,
    break_it: impl FnOnce(&mut IdentityConfig),
    fix: impl FnOnce(&mut IdentityConfig),
) {
    break_it(&mut config);
    let refused = config.validate();
    assert_eq!(keys(&refused), [key]);
    assert!(
        refused[0].to_string().starts_with(key),
        "message does not name the key: {}",
        refused[0]
    );
    fix(&mut config);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn only_required_keys_give_the_scope_default_durations() {
    // Arrange
    let config = defaults();

    // Act
    let durations = [
        config.access_token_ttl,
        config.refresh_token_ttl,
        config.refresh_reuse_grace,
        config.session_cache_ttl,
        config.session_last_seen_interval,
        config.device_code_ttl,
        config.device_poll_interval,
        config.oauth_state_ttl,
        config.login_code_ttl,
        config.invite_ttl,
        config.setup_token_ttl,
        config.provider_timeout,
    ];

    // Assert
    assert_eq!(
        durations.map(|duration| duration.as_secs()),
        [
            15 * MINUTE,
            30 * DAY,
            30,
            2,
            MINUTE,
            10 * MINUTE,
            5,
            10 * MINUTE,
            2 * MINUTE,
            7 * DAY,
            7 * DAY,
            10
        ]
    );
}

#[test]
fn only_required_keys_leave_the_optional_settings_off_or_empty() {
    // Arrange
    let config = defaults();

    // Act
    let optional = (
        config.allow_plain_http_dev,
        config.email_allowlist.len(),
        config.geoip_db_path,
        config.device_grant_ip_limit,
        config.invalid_token_ip_limit,
    );

    // Assert
    assert_eq!(optional, (false, 0, None, None, None));
}

#[test]
fn only_required_keys_give_one_provider_and_a_valid_section() {
    // Arrange
    let config = defaults();

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!((config.providers.len(), problems), (1, Vec::new()));
}

#[test]
fn no_provider_is_refused_naming_providers() {
    assert_refused_then_accepted(
        "providers",
        |config| config.providers.clear(),
        |config| config.providers = valid().providers,
    );
}

#[test]
fn provider_kind_facebook_is_refused_naming_the_kind() {
    assert_refused_then_accepted(
        "providers[0].kind",
        |config| config.providers[0].kind = "facebook".to_owned(),
        |config| config.providers[0].kind = "oidc".to_owned(),
    );
}

#[test]
fn relative_public_url_is_refused_naming_public_url() {
    assert_refused_then_accepted(
        "public_url",
        |config| config.public_url = "/sunnycloud".to_owned(),
        |config| config.public_url = "https://sunnycloud.test".to_owned(),
    );
}

#[test]
fn http_public_url_with_plain_http_dev_off_is_refused_naming_public_url() {
    assert_refused_then_accepted(
        "public_url",
        |config| config.public_url = "http://sunnycloud.test".to_owned(),
        |config| config.allow_plain_http_dev = true,
    );
}

#[test]
fn device_code_ttl_above_ten_minutes_is_refused_naming_the_key() {
    assert_refused_then_accepted(
        "device_code_ttl",
        |config| config.device_code_ttl = Duration::from_secs(10 * MINUTE + 1),
        |config| config.device_code_ttl = Duration::from_secs(10 * MINUTE),
    );
}

#[test]
fn session_cache_ttl_above_three_seconds_is_refused_naming_the_key() {
    assert_refused_then_accepted(
        "session_cache_ttl",
        |config| config.session_cache_ttl = Duration::from_millis(3001),
        |config| config.session_cache_ttl = Duration::from_secs(3),
    );
}

#[test]
fn every_unsafe_setting_at_once_is_reported_together() {
    // Arrange
    let mut config = valid();
    config.providers.clear();
    config.public_url = "/x".to_owned();
    config.device_code_ttl = Duration::from_secs(11 * MINUTE);
    config.session_cache_ttl = Duration::from_secs(4);

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(
        keys(&problems),
        [
            "public_url",
            "providers",
            "device_code_ttl",
            "session_cache_ttl"
        ]
    );
}

#[test]
fn refusal_reasons_are_out_of_range_for_ceilings() {
    // Arrange
    let mut config = valid();
    config.session_cache_ttl = Duration::from_secs(4);

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(
        problems,
        vec![KeyProblem::new(
            "session_cache_ttl",
            Reason::OutOfRange {
                min: "0s".to_owned(),
                max: "3s".to_owned()
            }
        )]
    );
}

fn apple_valid() -> IdentityConfig {
    let mut config = valid();
    config.providers = vec![TeamKey::generate().provider()];
    config
}

#[test]
fn an_apple_provider_with_its_key_team_id_and_key_id_is_valid() {
    // Arrange
    let config = apple_valid();

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(problems, Vec::new());
}

#[test]
fn an_apple_provider_without_its_key_file_is_refused_naming_the_provider_entry() {
    let good = apple_valid().providers;
    refused_then_accepted(
        apple_valid(),
        "providers[0].client_secret",
        |config| config.providers[0].client_secret = sc_config::Secret::new("  \n".to_owned()),
        |config| config.providers = good,
    );
}

#[test]
fn an_apple_provider_without_a_team_id_or_key_id_or_with_a_malformed_team_id_is_refused_naming_it()
{
    for (key, broken) in [
        ("providers[0].team_id", None),
        ("providers[0].team_id", Some("SHORT")),
        ("providers[0].team_id", Some("TEAM-23456")),
    ] {
        refused_then_accepted(
            apple_valid(),
            key,
            |config| config.providers[0].team_id = broken.map(str::to_owned),
            |config| config.providers[0].team_id = Some(TEAM_ID.to_owned()),
        );
    }
    refused_then_accepted(
        apple_valid(),
        "providers[0].key_id",
        |config| config.providers[0].key_id = None,
        |config| config.providers[0].key_id = Some(KEY_ID.to_owned()),
    );
}

#[test]
fn a_key_file_that_is_not_an_ec_key_refuses_the_start_naming_the_provider_entry_without_echoing_it()
{
    // Arrange
    let mut config = apple_valid();
    config.providers[0].client_secret = sc_config::Secret::new("not-a-key-file".to_owned());

    // Act
    let result = IdentityProvidersCheck::new(sc_testdb::clock_at_start(), &config);

    // Assert
    let Err(IdentityError::Config(problems)) = result else {
        panic!("expected a config refusal");
    };
    assert_eq!(keys(&problems), ["providers[0].client_secret"]);
    assert!(!problems[0].to_string().contains("not-a-key-file"));
}

#[test]
fn the_apple_keys_of_the_settings_make_an_apple_provider_after_github() {
    // Arrange
    let team = TeamKey::generate();
    let dir = tempfile::tempdir().expect("scratch dir");
    let file = dir.path().join("apple_p8");
    std::fs::write(&file, &team.p8).expect("write the key file");
    let file_path = file.to_string_lossy().into_owned();

    // Act
    let settings = load_settings(&[
        ("SUNNYCLOUD_APPLE_CLIENT_ID", SERVICES_ID),
        ("SUNNYCLOUD_APPLE_TEAM_ID", TEAM_ID),
        ("SUNNYCLOUD_APPLE_KEY_ID", KEY_ID),
        ("SUNNYCLOUD_APPLE_P8_FILE", file_path.as_str()),
    ]);
    let config = IdentityConfig::from_settings(&settings);

    // Assert
    let kinds: Vec<&str> = config.providers.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds, ["github", APPLE]);
    let apple = &config.providers[1];
    assert_eq!(
        (
            apple.client_id.as_str(),
            apple.team_id.as_deref(),
            apple.key_id.as_deref()
        ),
        (SERVICES_ID, Some(TEAM_ID), Some(KEY_ID))
    );
    assert_eq!(apple.client_secret.expose().as_str(), team.p8.trim_end());
    assert_eq!(config.validate(), Vec::new());
}
