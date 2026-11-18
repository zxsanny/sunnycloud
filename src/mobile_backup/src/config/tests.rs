use std::time::Duration;

use sc_config::{ClientSettings, ConfigError, ConfigSources, Reason, load};

use super::AndroidConfig;

fn sources(env: &[(&str, &str)]) -> ConfigSources {
    let mut sources = ConfigSources::default();
    for (name, value) in env {
        sources.env.insert((*name).to_owned(), (*value).to_owned());
    }
    sources
}

#[test]
fn defaults_are_the_values_of_17_section_10() {
    // Arrange
    let settings = load::<ClientSettings>(&sources(&[])).expect("defaults load");

    // Act
    let config = AndroidConfig::from_settings(&settings);

    // Assert
    assert_eq!(
        config,
        AndroidConfig {
            catchup_interval: Duration::from_secs(15 * 60),
            unmetered_only: true,
            charging_only: false,
            hash_ahead: 8,
        }
    );
}

#[test]
fn overrides_reach_the_view() {
    // Arrange
    let env = [
        ("SUNNYCLOUD_ANDROID_UNMETERED_ONLY", "false"),
        ("SUNNYCLOUD_ANDROID_CHARGING_ONLY", "true"),
        ("SUNNYCLOUD_ANDROID_HASH_AHEAD", "16"),
    ];
    let settings = load::<ClientSettings>(&sources(&env)).expect("overrides load");

    // Act
    let config = AndroidConfig::from_settings(&settings);

    // Assert
    assert!(!config.unmetered_only && config.charging_only && config.hash_ahead == 16);
}

#[test]
fn an_unknown_android_key_refuses_the_load_with_the_key_named() {
    // Arrange
    let env = [("SUNNYCLOUD_ANDROID_BOGUS", "1")];

    // Act
    let refusal = load::<ClientSettings>(&sources(&env)).expect_err("unknown key refuses");

    // Assert
    let ConfigError::Invalid(problems) = refusal else {
        panic!("expected an Invalid refusal, got {refusal:?}");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.key == "SUNNYCLOUD_ANDROID_BOGUS" && p.reason == Reason::Unknown),
        "{problems:?}"
    );
}

#[test]
fn a_hash_ahead_outside_its_range_is_refused() {
    // Arrange
    let env = [("SUNNYCLOUD_ANDROID_HASH_AHEAD", "0")];

    // Act
    let refusal = load::<ClientSettings>(&sources(&env)).expect_err("range binds");

    // Assert
    let ConfigError::Invalid(problems) = refusal else {
        panic!("expected an Invalid refusal, got {refusal:?}");
    };
    assert!(
        problems.iter().any(|p| p.key == "android_hash_ahead"),
        "{problems:?}"
    );
}
