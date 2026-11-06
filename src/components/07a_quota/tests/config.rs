//! AC-3: the quota section binds `default_quota`, 100 GiB when unset, and refuses a value it cannot parse.

mod support;

use sc_config::{ConfigError, KeyProblem, Reason};
use sunnycloud_quota::{MAX_LIMIT_BYTES, QuotaConfig};
use support::settings::load_settings;

const GIB: i64 = 1 << 30;

fn config_from(extra: &[(&str, &str)]) -> QuotaConfig {
    let settings = load_settings(extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    QuotaConfig::from_settings(&settings)
}

#[test]
fn unset_default_quota_is_100_gib() {
    // Arrange
    let no_quota_keys = [];

    // Act
    let config = config_from(&no_quota_keys);

    // Assert
    assert_eq!(config.default_quota_bytes, 100 * GIB);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn set_default_quota_replaces_the_default() {
    // Arrange
    let keys = [("SUNNYCLOUD_DEFAULT_QUOTA", "200GiB")];

    // Act
    let config = config_from(&keys);

    // Assert
    assert_eq!(config.default_quota_bytes, 200 * GIB);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn unparsable_default_quota_refuses_the_start_naming_the_key() {
    // Arrange
    let keys = [("SUNNYCLOUD_DEFAULT_QUOTA", "lots")];

    // Act
    let result = load_settings(&keys);

    // Assert
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refused start, got {result:?}");
    };
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].key, "default_quota");
    assert!(problems[0].to_string().starts_with("default_quota"));
}

#[test]
fn default_quota_above_the_limit_bound_is_refused_by_validation() {
    // Arrange
    let over = config_from(&[("SUNNYCLOUD_DEFAULT_QUOTA", "9007199254740993")]);

    // Act
    let problems = over.validate();

    // Assert
    assert_eq!(
        problems,
        vec![KeyProblem::new(
            "default_quota",
            Reason::OutOfRange {
                min: "0B".to_owned(),
                max: "8192TiB".to_owned()
            }
        )]
    );
}

#[test]
fn default_quota_at_the_limit_bound_is_accepted() {
    // Arrange
    let at_bound = config_from(&[("SUNNYCLOUD_DEFAULT_QUOTA", "8192TiB")]);

    // Act
    let problems = at_bound.validate();

    // Assert
    assert_eq!(at_bound.default_quota_bytes, MAX_LIMIT_BYTES);
    assert_eq!(problems, Vec::new());
}
