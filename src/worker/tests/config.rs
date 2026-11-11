//! AC-1 and AC-2 on the section itself.

use std::collections::BTreeMap;
use std::time::Duration;

use sc_config::{ConfigError, Reason};
use sunnycloud_worker::{WorkerError, load_settings, ops_addr};

mod support;

use support::{TOKEN, TokenFile, token_env};

const MARKER: &str = "Zq7fK2mXw9PaB4nTc8RvL1sYh6JdE3uGo5NiA0xCkMbWtQyV";

#[test]
fn ac1_defaults_equal_the_documented_values() {
    let token = TokenFile::new(TOKEN);
    let settings = load_settings(token_env(&token)).expect("defaults load");

    assert_eq!(settings.worker_api_url, "http://api:8081");
    assert_eq!(
        settings.worker_claim_poll_interval.get(),
        Duration::from_millis(500)
    );
    assert_eq!(
        settings.worker_status_interval.get(),
        Duration::from_secs(30)
    );
    assert_eq!(
        settings.decoder_silence_timeout.get(),
        Duration::from_secs(20)
    );
    assert_eq!(settings.decoder_cancel_grace.get(), Duration::from_secs(5));
    assert_eq!(settings.worker_max_input_photo.bytes(), 256 << 20);
    assert_eq!(settings.worker_max_input_video.bytes(), 10 << 30);
    assert_eq!(settings.worker_scratch_general_cap.bytes(), 24 << 30);
    assert_eq!(
        settings.worker_shutdown_grace.get(),
        Duration::from_secs(30)
    );
    assert_eq!(settings.worker_ops_listen, ":9090");
    assert_eq!(settings.nvenc, "off");
    assert_eq!(
        ops_addr(&settings).expect("default address"),
        "0.0.0.0:9090".parse().unwrap()
    );
}

#[test]
fn ac1_an_invalid_duration_and_a_missing_token_are_both_listed() {
    let env = BTreeMap::from([(
        "SUNNYCLOUD_WORKER_STATUS_INTERVAL".to_owned(),
        "soon".to_owned(),
    )]);

    let error = load_settings(env).expect_err("must refuse");

    let WorkerError::Config(ConfigError::Invalid(problems)) = &error else {
        panic!("expected a listed refusal, got {error:?}");
    };
    let find = |key: &str| problems.iter().find(|problem| problem.key == key);
    assert_eq!(
        find("worker_status_interval").map(|problem| &problem.reason),
        Some(&Reason::WrongType)
    );
    assert_eq!(
        find("worker_api_token").map(|problem| &problem.reason),
        Some(&Reason::MissingRequired)
    );
    assert_eq!(error.exit_code(), 3);
}

#[test]
fn ac1_an_unusable_ops_address_refuses_start() {
    let token = TokenFile::new(TOKEN);
    let mut env = token_env(&token);
    env.insert(
        "SUNNYCLOUD_WORKER_OPS_LISTEN".to_owned(),
        "nowhere".to_owned(),
    );

    let error = load_settings(env).expect_err("must refuse");

    assert!(matches!(error, WorkerError::OpsListen), "{error:?}");
    assert_eq!(error.exit_code(), 3);
}

#[test]
fn log_keys_are_read_by_the_worker() {
    let token = TokenFile::new(TOKEN);
    let mut env = token_env(&token);
    env.insert("SUNNYCLOUD_LOG".to_owned(), "json".to_owned());
    env.insert("SUNNYCLOUD_LOG_LEVEL".to_owned(), "debug".to_owned());

    let settings = load_settings(env).expect("loads");

    assert_eq!(settings.log, "json");
    assert_eq!(settings.log_level, "debug");
}

#[test]
fn ac2_debug_output_of_the_section_shows_only_a_masked_token() {
    assert_eq!(MARKER.len(), 48);
    let token = TokenFile::new(MARKER);
    let settings = load_settings(token_env(&token)).expect("loads");

    let printed = format!("{settings:?}");
    let pretty = format!("{settings:#?}");

    for text in [&printed, &pretty] {
        assert!(!text.contains(MARKER), "the marker leaked");
        assert!(
            text.contains("worker_api_token: ***"),
            "masked value missing"
        );
    }
    assert_eq!(settings.worker_api_token.expose(), MARKER);
}

#[test]
fn ac2_a_refusal_never_contains_the_marker() {
    let env = BTreeMap::from([
        ("SUNNYCLOUD_WORKER_API_TOKEN".to_owned(), MARKER.to_owned()),
        (
            "SUNNYCLOUD_WORKER_SHUTDOWN_GRACE".to_owned(),
            "never".to_owned(),
        ),
    ]);

    let error = load_settings(env).expect_err("must refuse");

    assert!(!error.to_string().contains(MARKER));
    assert!(!format!("{error:?}").contains(MARKER));
}

#[test]
fn nvenc_accepts_on_and_off_and_refuses_every_other_value() {
    for value in ["on", "off"] {
        // Arrange
        let token = TokenFile::new(TOKEN);
        let mut env = token_env(&token);
        env.insert("SUNNYCLOUD_NVENC".to_owned(), value.to_owned());

        // Act
        let settings = load_settings(env);

        // Assert
        assert!(settings.is_ok(), "{value}");
    }
    for value in ["", "ON", "true", "1", "auto"] {
        // Arrange
        let token = TokenFile::new(TOKEN);
        let mut env = token_env(&token);
        env.insert("SUNNYCLOUD_NVENC".to_owned(), value.to_owned());

        // Act
        let error = load_settings(env).expect_err("an invalid nvenc value");

        // Assert
        assert!(matches!(error, WorkerError::Nvenc), "{value}: {error:?}");
        assert_eq!(error.exit_code(), 3);
    }
}
