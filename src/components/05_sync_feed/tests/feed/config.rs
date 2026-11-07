//! AC-3: the feed section takes its defaults, refuses a wait outside 1 s to 55 s naming `longpoll_wait_max`,
//! and accepts the bounds.

use std::time::Duration;

use sc_config::{ConfigError, KeyProblem, Reason};
use sunnycloud_sync_feed::FeedConfig;

use crate::support::load_settings;

fn config_from(extra: &[(&str, &str)]) -> FeedConfig {
    let settings = load_settings(extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    FeedConfig::from_settings(&settings)
}

fn refused_keys(extra: &[(&str, &str)]) -> Vec<String> {
    let Err(ConfigError::Invalid(problems)) = load_settings(extra) else {
        panic!("expected a refused start for {extra:?}");
    };
    problems.into_iter().map(|problem| problem.key).collect()
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
    let no_feed_keys = [];

    // Act
    let config = config_from(&no_feed_keys);

    // Assert
    assert_eq!(
        config,
        FeedConfig {
            longpoll_wait_max: Duration::from_secs(30),
            longpoll_auth_recheck: Duration::from_secs(2),
            feed_safety_recheck: Duration::from_secs(10),
            feed_page_limit_default: 500,
            feed_page_limit_max: None,
            max_holds_per_principal: 4,
        }
    );
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn set_keys_replace_the_defaults() {
    // Arrange
    let overrides = [
        ("SUNNYCLOUD_LONGPOLL_WAIT_MAX", "45s"),
        ("SUNNYCLOUD_LONGPOLL_AUTH_RECHECK", "3s"),
        ("SUNNYCLOUD_FEED_SAFETY_RECHECK", "20s"),
        ("SUNNYCLOUD_FEED_PAGE_LIMIT_DEFAULT", "100"),
        ("SUNNYCLOUD_FEED_PAGE_LIMIT_MAX", "2000"),
        ("SUNNYCLOUD_MAX_HOLDS_PER_PRINCIPAL", "8"),
    ];

    // Act
    let config = config_from(&overrides);

    // Assert
    assert_eq!(config.longpoll_wait_max, Duration::from_secs(45));
    assert_eq!(config.longpoll_auth_recheck, Duration::from_secs(3));
    assert_eq!(config.feed_safety_recheck, Duration::from_secs(20));
    assert_eq!(config.feed_page_limit_default, 100);
    assert_eq!(config.feed_page_limit_max, Some(2000));
    assert_eq!(config.max_holds_per_principal, 8);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn wait_below_one_second_or_above_55_seconds_refuses_the_start_naming_the_key() {
    // Arrange
    let too_short = [("SUNNYCLOUD_LONGPOLL_WAIT_MAX", "500ms")];
    let too_long = [("SUNNYCLOUD_LONGPOLL_WAIT_MAX", "56s")];

    // Act
    let short_keys = refused_keys(&too_short);
    let long_keys = refused_keys(&too_long);

    // Assert
    assert_eq!(short_keys, ["longpoll_wait_max"]);
    assert_eq!(long_keys, ["longpoll_wait_max"]);
}

#[test]
fn wait_of_one_second_and_of_55_seconds_is_accepted() {
    // Arrange
    let low = [("SUNNYCLOUD_LONGPOLL_WAIT_MAX", "1s")];
    let high = [("SUNNYCLOUD_LONGPOLL_WAIT_MAX", "55s")];

    // Act
    let (low, high) = (config_from(&low), config_from(&high));

    // Assert
    assert_eq!(low.longpoll_wait_max, Duration::from_secs(1));
    assert_eq!(high.longpoll_wait_max, Duration::from_secs(55));
    assert_eq!(low.validate(), Vec::new());
    assert_eq!(high.validate(), Vec::new());
}

#[test]
fn validation_refuses_a_wait_outside_the_range_in_any_environment() {
    // Arrange
    let config = |wait: Duration| FeedConfig {
        longpoll_wait_max: wait,
        ..config_from(&[])
    };

    // Act
    let short = config(Duration::from_millis(500)).validate();
    let long = config(Duration::from_secs(56)).validate();

    // Assert
    for problems in [short, long] {
        assert_eq!(
            problems,
            vec![KeyProblem::new(
                "longpoll_wait_max",
                Reason::OutOfRange {
                    min: "1s".to_owned(),
                    max: "55s".to_owned()
                }
            )]
        );
    }
}

#[test]
fn validation_refuses_zero_intervals_zero_counts_and_a_page_ceiling_below_the_default() {
    // Arrange
    let broken = FeedConfig {
        longpoll_wait_max: Duration::from_secs(30),
        longpoll_auth_recheck: Duration::ZERO,
        feed_safety_recheck: Duration::ZERO,
        feed_page_limit_default: 0,
        feed_page_limit_max: Some(0),
        max_holds_per_principal: 0,
    };
    let ceiling_below_default = FeedConfig {
        feed_page_limit_default: 500,
        feed_page_limit_max: Some(100),
        ..config_from(&[])
    };

    // Act
    let problems = broken.validate();
    let ceiling_problems = ceiling_below_default.validate();

    // Assert
    assert_eq!(
        keys(&problems),
        [
            "longpoll_auth_recheck",
            "feed_safety_recheck",
            "feed_page_limit_default",
            "max_holds_per_principal",
            "feed_page_limit_max",
        ]
    );
    assert_eq!(keys(&ceiling_problems), ["feed_page_limit_max"]);
}
