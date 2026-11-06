//! AC-4 (defaults) and AC-5 (start validation) of the identity section. Defaults are read, never waited for.

mod support;

use std::time::Duration;

use sc_config::{KeyProblem, Reason};
use sunnycloud_identity::IdentityConfig;
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
    let mut config = valid();
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
