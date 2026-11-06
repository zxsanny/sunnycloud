//! AC-1: the admin section binds with its defaults, and the deadline range refuses a start.

mod support;

use std::time::Duration;

use sc_config::{ConfigError, Environment, KeyProblem, Reason};
use sunnycloud_admin::AdminConfig;
use support::settings::load_settings;

fn config_from(extra: &[(&str, &str)]) -> AdminConfig {
    let settings = load_settings(Environment::Production, extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    AdminConfig::from_settings(&settings)
}

fn refused_keys(extra: &[(&str, &str)]) -> Vec<KeyProblem> {
    match load_settings(Environment::Production, extra) {
        Err(ConfigError::Invalid(problems)) => problems,
        other => panic!("expected a refused start, got {other:?}"),
    }
}

#[test]
fn no_admin_keys_give_the_documented_defaults() {
    // Arrange
    let no_admin_keys = [];

    // Act
    let config = config_from(&no_admin_keys);

    // Assert
    assert_eq!(config.account_delete_batch_size, 5000);
    assert_eq!(config.account_delete_interval, Duration::from_secs(5));
    assert_eq!(
        config.account_delete_alert_after,
        Duration::from_secs(10 * 60)
    );
    assert_eq!(config.account_delete_deadline, Duration::from_secs(15 * 60));
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn keys_without_a_documented_default_stay_unset() {
    // Arrange
    let no_admin_keys = [];

    // Act
    let config = config_from(&no_admin_keys);

    // Assert
    assert_eq!(config.health_check_timeout, None);
    assert_eq!(config.admin_list_page_size, None);
}

#[test]
fn set_keys_replace_the_defaults() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_ACCOUNT_DELETE_BATCH_SIZE", "50"),
        ("SUNNYCLOUD_ACCOUNT_DELETE_INTERVAL", "200ms"),
        ("SUNNYCLOUD_HEALTH_CHECK_TIMEOUT", "300ms"),
        ("SUNNYCLOUD_ADMIN_LIST_PAGE_SIZE", "20"),
    ];

    // Act
    let config = config_from(&keys);

    // Assert
    assert_eq!(config.account_delete_batch_size, 50);
    assert_eq!(config.account_delete_interval, Duration::from_millis(200));
    assert_eq!(
        config.health_check_timeout,
        Some(Duration::from_millis(300))
    );
    assert_eq!(config.admin_list_page_size, Some(20));
}

#[test]
fn deadline_of_30_seconds_refuses_the_start_naming_the_key() {
    // Arrange
    let keys = [("SUNNYCLOUD_ACCOUNT_DELETE_DEADLINE", "30s")];

    // Act
    let problems = refused_keys(&keys);

    // Assert
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].key, "account_delete_deadline");
    assert!(
        problems[0]
            .to_string()
            .starts_with("account_delete_deadline")
    );
}

#[test]
fn deadline_of_61_minutes_refuses_the_start_naming_the_key() {
    // Arrange
    let keys = [("SUNNYCLOUD_ACCOUNT_DELETE_DEADLINE", "61m")];

    // Act
    let problems = refused_keys(&keys);

    // Assert
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].key, "account_delete_deadline");
    assert!(matches!(problems[0].reason, Reason::OutOfRange { .. }));
}

#[test]
fn deadline_at_both_bounds_is_accepted() {
    // Arrange
    let at_floor = [("SUNNYCLOUD_ACCOUNT_DELETE_DEADLINE", "1m")];
    let at_ceiling = [("SUNNYCLOUD_ACCOUNT_DELETE_DEADLINE", "60m")];

    // Act
    let floor = config_from(&at_floor);
    let ceiling = config_from(&at_ceiling);

    // Assert
    assert_eq!(floor.account_delete_deadline, Duration::from_secs(60));
    assert_eq!(ceiling.account_delete_deadline, Duration::from_secs(3600));
}

#[test]
fn zero_counts_and_a_zero_timeout_are_refused_naming_each_key() {
    // Arrange
    let mut config = config_from(&[]);
    config.account_delete_batch_size = 0;
    config.admin_list_page_size = Some(0);
    config.health_check_timeout = Some(Duration::ZERO);

    // Act
    let problems = config.validate();

    // Assert
    let keys: Vec<&str> = problems
        .iter()
        .map(|problem| problem.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "account_delete_batch_size",
            "admin_list_page_size",
            "health_check_timeout"
        ]
    );
}

#[test]
fn a_count_that_does_not_fit_the_port_parameter_is_refused() {
    // Arrange
    let mut config = config_from(&[]);
    config.account_delete_batch_size = u64::from(u32::MAX) + 1;

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].key, "account_delete_batch_size");
}
