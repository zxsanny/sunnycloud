//! AC-5: the tree section takes the defaults of the specification, honours millisecond overrides outside
//! production, and refuses values that would stall a loop. Settings are loaded through `sc_config` exactly as
//! the server loads them.

use std::sync::Arc;
use std::time::Duration;

use sc_config::{ApiSettings, ConfigError, ConfigSources, Environment, KeyProblem, Reason, load};
use sunnycloud_tree::{DEFAULT_MAINTENANCE_INTERVAL, DEFAULT_PURGE_BATCH_SIZE, TreeConfig};
use uuid::Uuid;

const DAY: u64 = 86_400;

/// A load with every required key; `extra` pairs go on top. Secrets come from files of a folder that exists
/// for the load only.
fn load_settings(
    environment: Environment,
    extra: &[(&str, &str)],
) -> Result<Arc<ApiSettings>, ConfigError> {
    let secrets_dir = std::env::temp_dir().join(format!("sc-tree-secrets-{}", Uuid::new_v4()));
    std::fs::create_dir(&secrets_dir).expect("create the secrets folder");
    let secrets = [
        ("provider_github_client_secret", "gh-secret"),
        ("db_api_url", "postgres://api@db/sunnycloud"),
        ("share_grant_key", "grant-key"),
        ("worker_api_token", "worker-token"),
        ("master_key_file", "/run/secrets/sentinel-master-key"),
    ];
    for (name, value) in secrets {
        std::fs::write(secrets_dir.join(name), value).expect("write a secret file");
    }
    let mut sources = ConfigSources {
        secrets_dir: secrets_dir.clone(),
        environment,
        ..ConfigSources::default()
    };
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
        ("SUNNYCLOUD_BLOB_ROOT", "/var/lib/sunnycloud/blobs"),
    ];
    for (key, value) in required.iter().chain(extra) {
        sources.env.insert((*key).to_owned(), (*value).to_owned());
    }
    let loaded = load::<ApiSettings>(&sources);
    std::fs::remove_dir_all(&secrets_dir).expect("remove the secrets folder");
    loaded
}

fn config_from(environment: Environment, extra: &[(&str, &str)]) -> TreeConfig {
    let settings = load_settings(environment, extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    TreeConfig::from_settings(&settings)
}

fn valid() -> TreeConfig {
    config_from(Environment::Production, &[])
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
    break_it: impl FnOnce(&mut TreeConfig),
    fix: impl FnOnce(&mut TreeConfig),
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
fn unset_keys_take_the_specified_defaults() {
    // Arrange
    let no_tree_keys = [];

    // Act
    let config = config_from(Environment::Production, &no_tree_keys);

    // Assert
    assert_eq!(
        config,
        TreeConfig {
            tombstone_horizon: Duration::from_secs(90 * DAY),
            listing_default_limit: 1000,
            listing_max_limit: 1000,
            cascade_batch_size: 5000,
            purge_batch_size: 5000,
            tree_maintenance_interval: Duration::from_secs(30),
            conflict_copy_time_zone: "UTC".to_owned(),
        }
    );
    assert_eq!(DEFAULT_PURGE_BATCH_SIZE, 5000);
    assert_eq!(DEFAULT_MAINTENANCE_INTERVAL, Duration::from_secs(30));
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn tombstone_horizon_of_two_seconds_is_honoured_outside_production() {
    // Arrange
    let extra = [("SUNNYCLOUD_TOMBSTONE_HORIZON", "2s")];

    // Act
    let config = config_from(Environment::Test, &extra);

    // Assert
    assert_eq!(config.tombstone_horizon, Duration::from_secs(2));
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn tombstone_horizon_of_two_seconds_in_production_is_refused_naming_the_key() {
    // Arrange
    let extra = [("SUNNYCLOUD_TOMBSTONE_HORIZON", "2s")];

    // Act
    let result = load_settings(Environment::Production, &extra);

    // Assert
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(keys(&problems), ["tombstone_horizon"]);
    assert!(problems[0].to_string().starts_with("tombstone_horizon"));
}

#[test]
fn every_key_accepts_an_override() {
    // Arrange
    let extra = [
        ("SUNNYCLOUD_TOMBSTONE_HORIZON", "150ms"),
        ("SUNNYCLOUD_LISTING_DEFAULT_LIMIT", "10"),
        ("SUNNYCLOUD_LISTING_MAX_LIMIT", "50"),
        ("SUNNYCLOUD_CASCADE_BATCH_SIZE", "7"),
        ("SUNNYCLOUD_PURGE_BATCH_SIZE", "9"),
        ("SUNNYCLOUD_TREE_MAINTENANCE_INTERVAL", "250ms"),
        ("SUNNYCLOUD_CONFLICT_COPY_TIME_ZONE", " UTC "),
    ];

    // Act
    let config = config_from(Environment::Test, &extra);

    // Assert
    assert_eq!(
        config,
        TreeConfig {
            tombstone_horizon: Duration::from_millis(150),
            listing_default_limit: 10,
            listing_max_limit: 50,
            cascade_batch_size: 7,
            purge_batch_size: 9,
            tree_maintenance_interval: Duration::from_millis(250),
            conflict_copy_time_zone: " UTC ".to_owned(),
        }
    );
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn a_zero_limit_or_batch_is_refused() {
    type Set = fn(&mut TreeConfig, u64);
    let cases: [(&str, Set); 3] = [
        ("listing_default_limit", |c, v| c.listing_default_limit = v),
        ("cascade_batch_size", |c, v| c.cascade_batch_size = v),
        ("purge_batch_size", |c, v| c.purge_batch_size = v),
    ];

    for (key, set) in cases {
        assert_refused_then_accepted(key, |config| set(config, 0), |config| set(config, 1));
    }
}

#[test]
fn a_zero_maintenance_interval_is_refused() {
    assert_refused_then_accepted(
        "tree_maintenance_interval",
        |config| config.tree_maintenance_interval = Duration::ZERO,
        |config| config.tree_maintenance_interval = Duration::from_millis(1),
    );
}

#[test]
fn a_maximum_below_the_default_limit_is_refused() {
    assert_refused_then_accepted(
        "listing_max_limit",
        |config| config.listing_max_limit = 999,
        |config| config.listing_max_limit = 1000,
    );
}

#[test]
fn a_blank_time_zone_is_refused() {
    assert_refused_then_accepted(
        "conflict_copy_time_zone",
        |config| config.conflict_copy_time_zone = " ".to_owned(),
        |config| config.conflict_copy_time_zone = "UTC".to_owned(),
    );
}

#[test]
fn a_time_zone_other_than_utc_is_refused() {
    assert_refused_then_accepted(
        "conflict_copy_time_zone",
        |config| config.conflict_copy_time_zone = "Europe/Kyiv".to_owned(),
        |config| config.conflict_copy_time_zone = "UTC".to_owned(),
    );
}

#[test]
fn a_zero_default_limit_with_a_maximum_names_both_keys() {
    // Arrange
    let mut config = valid();
    config.listing_default_limit = 0;
    config.listing_max_limit = 0;

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(
        keys(&problems),
        ["listing_default_limit", "listing_max_limit"]
    );
    assert!(matches!(problems[1].reason, Reason::OutOfRange { .. }));
}
