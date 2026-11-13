//! AC-1: the retention range is enforced at start in a release build; the rest of the section validates
//! its own rules. Settings are loaded through `sc_config` exactly as the server loads them.

use std::sync::Arc;
use std::time::Duration;

use sc_config::{ApiSettings, ConfigError, ConfigSources, KeyProblem, Reason, load};
use sunnycloud_blobstore::BlobstoreConfig;
use uuid::Uuid;

const DAY: u64 = 86_400;
const SECRET_PATH: &str = "/run/secrets/sentinel-master-key";

/// A release (production) load with every required key and `blob_root`; `extra` pairs go on top. Secrets come
/// from files of a folder that exists for the load only.
fn load_settings(extra: &[(&str, &str)]) -> Result<Arc<ApiSettings>, ConfigError> {
    let secrets_dir = std::env::temp_dir().join(format!("sc-blobstore-secrets-{}", Uuid::new_v4()));
    std::fs::create_dir(&secrets_dir).expect("create the secrets folder");
    let secrets = [
        ("provider_github_client_secret", "gh-secret"),
        ("db_api_url", "postgres://api@db/sunnycloud"),
        ("share_grant_key", "grant-key"),
        ("worker_api_token", "worker-token"),
        ("master_key_file", SECRET_PATH),
    ];
    for (name, value) in secrets {
        std::fs::write(secrets_dir.join(name), value).expect("write a secret file");
    }
    let mut sources = ConfigSources {
        secrets_dir: secrets_dir.clone(),
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

fn valid() -> BlobstoreConfig {
    let settings = load_settings(&[]).expect("the baseline settings load");
    BlobstoreConfig::from_settings(&settings)
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
    break_it: impl FnOnce(&mut BlobstoreConfig),
    fix: impl FnOnce(&mut BlobstoreConfig),
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
fn retention_of_91_days_in_a_release_build_is_refused_naming_retention() {
    // Arrange
    let extra = [("SUNNYCLOUD_RETENTION", "91d")];

    // Act
    let result = load_settings(&extra);

    // Assert
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(keys(&problems), ["retention"]);
    assert!(matches!(problems[0].reason, Reason::OutOfRange { .. }));
    assert!(problems[0].to_string().starts_with("retention"));
}

#[test]
fn retention_of_zero_and_of_90_days_is_accepted() {
    // Arrange
    let cases = [
        ("0s", Duration::ZERO),
        ("90d", Duration::from_secs(90 * DAY)),
    ];

    for (text, expected) in cases {
        // Act
        let settings = load_settings(&[("SUNNYCLOUD_RETENTION", text)])
            .unwrap_or_else(|error| panic!("retention {text} refused: {error}"));
        let config = BlobstoreConfig::from_settings(&settings);

        // Assert
        assert_eq!(config.retention, expected);
        assert_eq!(config.validate(), Vec::new());
    }
}

#[test]
fn the_section_takes_the_table_defaults() {
    // Arrange
    let config = valid();

    // Act
    let durations = [
        config.retention,
        config.orphan_sweep_age,
        config.orphan_sweep_interval,
        config.gc_interval,
        config.gc_claim_lease,
        config.gc_cycle_budget,
        config.scrub_interval,
        config.scrub_cycle_budget,
        config.blob_sync_coalesce_delay,
    ];

    // Assert
    assert_eq!(
        durations.map(|duration| duration.as_millis()),
        [
            7 * DAY as u128 * 1000,
            48 * 3600 * 1000,
            DAY as u128 * 1000,
            5 * 60 * 1000,
            10 * 60 * 1000,
            30 * 1000,
            7 * DAY as u128 * 1000,
            30 * 60 * 1000,
            0,
        ]
    );
    assert_eq!(config.orphan_mode, "delete");
    assert_eq!(config.blob_free_reserve, 10 * 1024 * 1024 * 1024);
    assert_eq!(config.blob_free_reserve_percent, 5);
    assert_eq!(config.scrub_max_rate, 50 * 1024 * 1024);
    assert_eq!(
        (
            config.gc_batch,
            config.scrub_pause_active_streams,
            config.blob_read_ahead_chunks
        ),
        (100, 8, 16)
    );
    assert!(config.blob_preallocate);
    assert_eq!(
        (
            config.blob_io_threads,
            config.master_key_ring,
            config.master_key_current
        ),
        (None, None, None)
    );
}

#[test]
fn a_missing_blob_root_is_refused() {
    assert_refused_then_accepted(
        "blob_root",
        |config| config.blob_root = None,
        |config| config.blob_root = Some("/var/lib/sunnycloud/blobs".to_owned()),
    );
}

#[test]
fn an_unknown_orphan_mode_is_refused() {
    assert_refused_then_accepted(
        "orphan_mode",
        |config| config.orphan_mode = "purge".to_owned(),
        |config| config.orphan_mode = "report".to_owned(),
    );
}

#[test]
fn a_zero_interval_or_budget_is_refused() {
    type Set = fn(&mut BlobstoreConfig, Duration);
    let cases: [(&str, Set); 6] = [
        ("orphan_sweep_interval", |c, d| c.orphan_sweep_interval = d),
        ("gc_interval", |c, d| c.gc_interval = d),
        ("gc_claim_lease", |c, d| c.gc_claim_lease = d),
        ("gc_cycle_budget", |c, d| c.gc_cycle_budget = d),
        ("scrub_interval", |c, d| c.scrub_interval = d),
        ("scrub_cycle_budget", |c, d| c.scrub_cycle_budget = d),
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
fn a_zero_count_or_rate_is_refused() {
    type Set = fn(&mut BlobstoreConfig, u64);
    let cases: [(&str, Set); 4] = [
        ("gc_batch", |c, n| c.gc_batch = n),
        ("scrub_max_rate", |c, n| c.scrub_max_rate = n),
        ("blob_read_ahead_chunks", |c, n| {
            c.blob_read_ahead_chunks = n
        }),
        ("blob_io_threads", |c, n| c.blob_io_threads = Some(n)),
    ];

    for (key, set) in cases {
        assert_refused_then_accepted(key, |config| set(config, 0), |config| set(config, 1));
    }
}

#[test]
fn a_malformed_key_ring_is_refused() {
    for ring in [
        "2",
        "2=",
        "x=/k",
        "0=/k",
        "-1=/k",
        "2=/a,2=/b",
        "2=/a,,3=/b",
    ] {
        assert_refused_then_accepted(
            "master_key_ring",
            |config| config.master_key_ring = Some(ring.to_owned()),
            |config| {
                config.master_key_ring = Some("2=/run/secrets/k2,3=/run/secrets/k3".to_owned())
            },
        );
    }
}

#[test]
fn a_current_key_the_ring_cannot_load_is_refused() {
    let ring = Some("2=/run/secrets/k2".to_owned());
    for current in [3, 0, 32_768] {
        assert_refused_then_accepted(
            "master_key_current",
            |config| {
                config.master_key_ring = ring.clone();
                config.master_key_current = Some(current);
            },
            |config| config.master_key_current = Some(2),
        );
    }
}

#[test]
fn the_key_in_the_file_is_current_key_one() {
    // Arrange
    let mut config = valid();
    config.master_key_current = Some(1);

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(problems, Vec::new());
}

#[test]
fn debug_of_the_section_does_not_print_the_key_file_setting() {
    // Arrange
    let config = valid();

    // Act
    let printed = format!("{config:?}");

    // Assert
    assert!(!printed.contains(SECRET_PATH), "{printed}");
    assert!(printed.contains("***"), "{printed}");
}
