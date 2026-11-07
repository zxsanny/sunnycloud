//! AC-5: the media section takes its defaults, refuses a lease or an attempt count outside its range naming the
//! key, and holds the worker tokens only as secrets read once at load.

use std::time::Duration;

use media_api::MediaConfig;
use sc_config::{ConfigError, KeyProblem, Reason};

use crate::support::{load_settings, load_settings_with_tokens};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

fn config_from(extra: &[(&str, &str)]) -> MediaConfig {
    let settings = load_settings(extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    MediaConfig::from_settings(&settings)
}

fn refused(extra: &[(&str, &str)]) -> Vec<KeyProblem> {
    let Err(ConfigError::Invalid(problems)) = load_settings(extra) else {
        panic!("expected a refused start for {extra:?}");
    };
    problems
}

fn keys(problems: &[KeyProblem]) -> Vec<&str> {
    problems
        .iter()
        .map(|problem| problem.key.as_str())
        .collect()
}

#[test]
fn unset_keys_take_the_defaults() {
    // Arrange
    let no_media_keys = [];

    // Act
    let config = config_from(&no_media_keys);

    // Assert
    assert_eq!(config.job_lease, Duration::from_secs(5 * 60));
    assert_eq!(config.job_attempts, 3);
    assert_eq!(config.job_retry_backoff, Duration::from_secs(30));
    assert_eq!(config.bulk_queue_threshold, 200);
    assert_eq!(config.job_terminal_ttl, Duration::from_secs(14 * 86_400));
    assert_eq!(config.job_reap_interval, Duration::from_secs(60));
    assert_eq!(config.derived_janitor_interval, Duration::from_secs(3600));
    assert_eq!(config.derived_max_thumb, 5 * MIB);
    assert_eq!(config.derived_max_preview, 20 * MIB);
    assert_eq!(config.derived_max_transcode, 10 * GIB);
    assert!(config.worker_api_token_next.is_none());
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn set_keys_replace_the_defaults() {
    // Arrange
    let overrides = [
        ("SUNNYCLOUD_JOB_LEASE", "10s"),
        ("SUNNYCLOUD_JOB_ATTEMPTS", "5"),
        ("SUNNYCLOUD_JOB_RETRY_BACKOFF", "1m"),
        ("SUNNYCLOUD_BULK_QUEUE_THRESHOLD", "50"),
        ("SUNNYCLOUD_JOB_TERMINAL_TTL", "7d"),
        ("SUNNYCLOUD_JOB_REAP_INTERVAL", "2m"),
        ("SUNNYCLOUD_DERIVED_JANITOR_INTERVAL", "2h"),
        ("SUNNYCLOUD_DERIVED_MAX_THUMB", "1MiB"),
        ("SUNNYCLOUD_DERIVED_MAX_PREVIEW", "8MiB"),
        ("SUNNYCLOUD_DERIVED_MAX_TRANSCODE", "2GiB"),
    ];

    // Act
    let config = config_from(&overrides);

    // Assert
    assert_eq!(config.job_lease, Duration::from_secs(10));
    assert_eq!(config.job_attempts, 5);
    assert_eq!(config.job_retry_backoff, Duration::from_secs(60));
    assert_eq!(config.bulk_queue_threshold, 50);
    assert_eq!(config.job_terminal_ttl, Duration::from_secs(7 * 86_400));
    assert_eq!(config.job_reap_interval, Duration::from_secs(120));
    assert_eq!(config.derived_janitor_interval, Duration::from_secs(7200));
    assert_eq!(config.derived_max_thumb, MIB);
    assert_eq!(config.derived_max_preview, 8 * MIB);
    assert_eq!(config.derived_max_transcode, 2 * GIB);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn a_lease_of_4_seconds_or_of_2_hours_refuses_the_start_naming_the_key() {
    // Arrange
    let too_short = [("SUNNYCLOUD_JOB_LEASE", "4s")];
    let too_long = [("SUNNYCLOUD_JOB_LEASE", "2h")];

    // Act
    let short_problems = refused(&too_short);
    let long_problems = refused(&too_long);

    // Assert
    assert_eq!(keys(&short_problems), ["job_lease"]);
    assert_eq!(keys(&long_problems), ["job_lease"]);
}

#[test]
fn a_lease_of_5_seconds_and_of_1_hour_is_accepted() {
    // Arrange
    let low = [("SUNNYCLOUD_JOB_LEASE", "5s")];
    let high = [("SUNNYCLOUD_JOB_LEASE", "1h")];

    // Act
    let (low, high) = (config_from(&low), config_from(&high));

    // Assert
    assert_eq!(low.job_lease, Duration::from_secs(5));
    assert_eq!(high.job_lease, Duration::from_secs(3600));
    assert_eq!(low.validate(), Vec::new());
    assert_eq!(high.validate(), Vec::new());
}

#[test]
fn an_attempt_count_of_0_or_of_11_refuses_the_start_naming_the_key() {
    // Arrange
    let none = [("SUNNYCLOUD_JOB_ATTEMPTS", "0")];
    let eleven = [("SUNNYCLOUD_JOB_ATTEMPTS", "11")];

    // Act
    let none_problems = refused(&none);
    let eleven_problems = refused(&eleven);

    // Assert
    assert_eq!(keys(&none_problems), ["job_attempts"]);
    assert_eq!(keys(&eleven_problems), ["job_attempts"]);
}

#[test]
fn an_attempt_count_of_1_and_of_10_is_accepted() {
    // Arrange
    let low = [("SUNNYCLOUD_JOB_ATTEMPTS", "1")];
    let high = [("SUNNYCLOUD_JOB_ATTEMPTS", "10")];

    // Act
    let (low, high) = (config_from(&low), config_from(&high));

    // Assert
    assert_eq!((low.job_attempts, high.job_attempts), (1, 10));
    assert_eq!(low.validate(), Vec::new());
    assert_eq!(high.validate(), Vec::new());
}

#[test]
fn validation_refuses_zero_durations_zero_caps_an_attempt_count_out_of_range_and_empty_tokens() {
    // Arrange
    let broken = MediaConfig {
        job_lease: Duration::ZERO,
        job_attempts: 11,
        job_retry_backoff: Duration::ZERO,
        job_terminal_ttl: Duration::ZERO,
        job_reap_interval: Duration::ZERO,
        derived_janitor_interval: Duration::ZERO,
        derived_max_thumb: 0,
        derived_max_preview: 0,
        derived_max_transcode: 0,
        worker_api_token: sc_config::Secret::new(String::new()),
        worker_api_token_next: Some(sc_config::Secret::new(String::new())),
        ..config_from(&[])
    };
    let attempts_zero = MediaConfig {
        job_attempts: 0,
        ..config_from(&[])
    };

    // Act
    let problems = broken.validate();
    let attempts_problems = attempts_zero.validate();

    // Assert
    assert_eq!(
        keys(&problems),
        [
            "job_lease",
            "job_retry_backoff",
            "job_terminal_ttl",
            "job_reap_interval",
            "derived_janitor_interval",
            "job_attempts",
            "derived_max_thumb",
            "derived_max_preview",
            "derived_max_transcode",
            "worker_api_token",
            "worker_api_token_next",
        ]
    );
    assert_eq!(
        attempts_problems,
        vec![KeyProblem::new(
            "job_attempts",
            Reason::OutOfRange {
                min: "1".to_owned(),
                max: "10".to_owned()
            }
        )]
    );
}

#[test]
fn tokens_are_read_once_at_load_and_print_as_stars() {
    // Arrange
    let tokens = [
        ("worker_api_token", "current-token-7f3a"),
        ("worker_api_token_next", "next-token-91bc"),
    ];

    // Act
    let settings = load_settings_with_tokens(&tokens).expect("settings load");
    let config = MediaConfig::from_settings(&settings);
    let printed = format!("{config:?} {:?}", settings.worker_api_token);

    // Assert
    assert_eq!(config.worker_api_token.expose(), "current-token-7f3a");
    assert_eq!(
        config
            .worker_api_token_next
            .as_ref()
            .map(|t| t.expose().as_str()),
        Some("next-token-91bc")
    );
    assert!(printed.contains("***"));
    for secret in ["current-token-7f3a", "next-token-91bc"] {
        assert!(!printed.contains(secret), "{printed}");
    }
}

#[test]
fn a_missing_current_token_refuses_the_start_naming_the_key() {
    // Arrange
    let missing = [(
        "SUNNYCLOUD_WORKER_API_TOKEN_FILE",
        "/nonexistent/worker_api_token",
    )];

    // Act
    let result = load_settings(&missing);

    // Assert
    let Err(error) = result else {
        panic!("a missing token file must refuse the start");
    };
    let printed = error.to_string();
    assert!(printed.contains("worker_api_token"), "{printed}");
}
