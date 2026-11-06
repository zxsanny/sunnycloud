//! AC-4: the transfer section takes its defaults, applies an override (400 ms for the session TTL), refuses
//! values that would stall the component, and no key changes the 2 MiB block size.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use sc_config::{ApiSettings, ConfigError, ConfigSources, Environment, KeyProblem, Reason, load};
use sunnycloud_transfer::{BLOCK_SIZE, TransferConfig};
use uuid::Uuid;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// An API-section load with every required key; `extra` pairs go on top. Secrets come from files of a folder
/// that exists for the load only.
fn load_in(
    environment: Environment,
    extra: &[(&str, &str)],
) -> Result<Arc<ApiSettings>, ConfigError> {
    let secrets_dir = std::env::temp_dir().join(format!("sc-transfer-secrets-{}", Uuid::new_v4()));
    fs::create_dir(&secrets_dir).expect("create the secrets folder");
    for name in [
        "provider_github_client_secret",
        "db_api_url",
        "share_grant_key",
        "worker_api_token",
        "master_key_file",
    ] {
        fs::write(secrets_dir.join(name), "sample-secret").expect("write a secret file");
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
    fs::remove_dir_all(&secrets_dir).expect("remove the secrets folder");
    loaded
}

/// The test environment has no duration floors.
fn load_settings(extra: &[(&str, &str)]) -> Result<Arc<ApiSettings>, ConfigError> {
    load_in(Environment::Test, extra)
}

fn defaults() -> TransferConfig {
    let settings = load_settings(&[]).expect("the baseline settings load");
    TransferConfig::from_settings(&settings)
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
    break_it: impl FnOnce(&mut TransferConfig),
    fix: impl FnOnce(&mut TransferConfig),
) {
    let mut config = defaults();
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
fn unset_keys_take_the_documented_defaults() {
    // Arrange
    let expected = TransferConfig {
        upload_session_ttl: Duration::from_secs(24 * 3600),
        max_declared_size: 50 * GIB,
        max_open_sessions: 8,
        max_staged: 1000,
        checkpoint_bytes: 8 * MIB,
        checkpoint_interval: Duration::from_secs(1),
        max_lanes: 4,
        max_inflight_blocks_per_account: 32,
        batch_max_entries: 500,
        commit_receipt_ttl: Duration::from_secs(48 * 3600),
        max_buffered_block_bytes: GIB,
        session_reap_interval: Duration::from_secs(60),
        receipt_janitor_interval: Duration::from_secs(3600),
    };

    // Act
    let config = defaults();

    // Assert
    assert_eq!(config, expected);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn a_session_ttl_of_400_ms_overrides_the_default() {
    // Arrange
    let extra = [("SUNNYCLOUD_UPLOAD_SESSION_TTL", "400ms")];

    // Act
    let settings = load_settings(&extra).expect("the override loads");
    let config = TransferConfig::from_settings(&settings);

    // Assert
    assert_eq!(config.upload_session_ttl, Duration::from_millis(400));
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn the_account_in_flight_cap_follows_sessions_times_lanes() {
    // Arrange
    let extra = [
        ("SUNNYCLOUD_MAX_OPEN_SESSIONS", "3"),
        ("SUNNYCLOUD_MAX_LANES", "2"),
    ];

    // Act
    let settings = load_settings(&extra).expect("the overrides load");
    let config = TransferConfig::from_settings(&settings);

    // Assert
    assert_eq!(config.max_inflight_blocks_per_account, 6);
}

#[test]
fn no_key_changes_the_block_size() {
    // Arrange
    let extra = [("SUNNYCLOUD_BLOCK_SIZE", "1MiB")];

    // Act
    let result = load_settings(&extra);

    // Assert
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].reason, Reason::Unknown);
    assert_eq!(BLOCK_SIZE, 2 * 1024 * 1024);
}

#[test]
fn a_session_ttl_below_a_minute_is_refused_in_production() {
    // Arrange
    let extra = [("SUNNYCLOUD_UPLOAD_SESSION_TTL", "400ms")];

    // Act
    let result = load_in(Environment::Production, &extra);

    // Assert
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(keys(&problems), ["upload_session_ttl"]);
}

#[test]
fn a_zero_interval_or_ttl_is_refused() {
    type Set = fn(&mut TransferConfig, Duration);
    let cases: [(&str, Set); 5] = [
        ("upload_session_ttl", |c, d| c.upload_session_ttl = d),
        ("checkpoint_interval", |c, d| c.checkpoint_interval = d),
        ("commit_receipt_ttl", |c, d| c.commit_receipt_ttl = d),
        ("session_reap_interval", |c, d| c.session_reap_interval = d),
        ("receipt_janitor_interval", |c, d| {
            c.receipt_janitor_interval = d;
        }),
    ];

    for (key, set) in cases {
        assert_refused_then_accepted(
            key,
            |config| set(config, Duration::ZERO),
            |config| set(config, Duration::from_millis(1)),
        );
    }
}

#[test]
fn a_zero_count_or_byte_threshold_is_refused() {
    type Set = fn(&mut TransferConfig, u64);
    let cases: [(&str, Set); 5] = [
        ("max_open_sessions", |c, v| c.max_open_sessions = v),
        ("max_staged", |c, v| c.max_staged = v),
        ("checkpoint_bytes", |c, v| c.checkpoint_bytes = v),
        ("max_lanes", |c, v| c.max_lanes = v),
        ("batch_max_entries", |c, v| c.batch_max_entries = v),
    ];

    for (key, set) in cases {
        assert_refused_then_accepted(key, |config| set(config, 0), |config| set(config, 1));
    }
}

#[test]
fn a_declared_size_the_block_count_column_cannot_hold_is_refused() {
    let limit = i32::MAX as u64 * BLOCK_SIZE as u64;

    assert_refused_then_accepted(
        "max_declared_size",
        |config| config.max_declared_size = limit + 1,
        |config| config.max_declared_size = limit,
    );
    assert_refused_then_accepted(
        "max_declared_size",
        |config| config.max_declared_size = 0,
        |config| config.max_declared_size = 1,
    );
}

#[test]
fn a_buffer_smaller_than_one_block_is_refused() {
    assert_refused_then_accepted(
        "max_buffered_block_bytes",
        |config| config.max_buffered_block_bytes = BLOCK_SIZE as u64 - 1,
        |config| config.max_buffered_block_bytes = BLOCK_SIZE as u64,
    );
}
