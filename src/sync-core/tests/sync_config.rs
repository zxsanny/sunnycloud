//! AC-3 of AZ-3967: the `sync` section binds (defaults, file, environment), validates, and reloads.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use sc_config::{
    ClientSettings, ClientSettingsHandle, ConfigError, ConfigSources, Environment, KeyProblem,
    Reason, load,
};
use sync_core::SyncConfig;

struct ConfigFile(PathBuf);

impl ConfigFile {
    fn write(name: &str, json: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sync_core_cfg_{}_{name}.json", std::process::id()));
        fs::write(&path, json).expect("write config file");
        Self(path)
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            eprintln!("could not remove {:?}: {error}", self.0);
        }
    }
}

const BUILD_KEYS: &str = r#""sync_server_url": "https://sunnycloud.test", "sync_client_family": "desktop", "sync_client_version": "1.2.3""#;

fn sources(file: Option<&ConfigFile>, env: &[(&str, &str)]) -> ConfigSources {
    let mut sources = ConfigSources::default();
    sources.files.extend(file.map(|f| f.0.clone()));
    for (name, value) in env {
        sources.env.insert((*name).to_owned(), (*value).to_owned());
    }
    sources
}

fn config_of(sources: &ConfigSources) -> Result<SyncConfig, ConfigError> {
    let settings = load::<ClientSettings>(sources)?;
    SyncConfig::from_settings(&settings)
}

fn problems(result: Result<SyncConfig, ConfigError>) -> Vec<KeyProblem> {
    match result {
        Err(ConfigError::Invalid(problems)) => problems,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn environment_overrides_the_file_and_every_other_key_keeps_its_default() {
    let file = ConfigFile::write(
        "bind",
        &format!("{{{BUILD_KEYS}, \"sync_retry_base\": \"700ms\"}}"),
    );
    let sources = sources(Some(&file), &[("SUNNYCLOUD_SYNC_RETRY_BASE", "2s")]);

    let config = config_of(&sources).expect("valid section");

    assert_eq!(config.retry_base, Duration::from_secs(2));
    assert_eq!(config.server_url, "https://sunnycloud.test");
    assert_eq!(config.client_family, "desktop");
    assert_eq!(config.client_version, "1.2.3");
    assert_eq!(config.retry_max, Duration::from_secs(30));
    assert_eq!(config.max_attempts_per_block, 8);
    assert_eq!(config.connect_timeout, Duration::from_secs(10));
    assert_eq!(config.request_idle_timeout, Duration::from_secs(60));
    assert_eq!(config.longpoll_wait, Duration::from_secs(30));
    assert_eq!(config.longpoll_client_margin, Duration::from_secs(10));
    assert_eq!(config.h2_stream_window, 8 << 20);
    assert_eq!(config.h2_conn_window, 32 << 20);
    assert_eq!(config.keepalive_interval, Duration::from_secs(20));
    assert_eq!(config.hash_cpu_share, 25);
    assert_eq!(config.hash_checkpoint_blocks, 32);
    assert_eq!(config.batch_max_bytes_in_flight, 64 << 20);
}

#[test]
fn the_file_value_applies_when_the_environment_is_silent() {
    let file = ConfigFile::write(
        "file",
        &format!("{{{BUILD_KEYS}, \"sync_retry_base\": \"700ms\"}}"),
    );

    let config = config_of(&sources(Some(&file), &[])).expect("valid section");

    assert_eq!(config.retry_base, Duration::from_millis(700));
}

#[test]
fn an_out_of_range_value_is_reported_with_its_key() {
    let file = ConfigFile::write("range", &format!("{{{BUILD_KEYS}}}"));
    let sources = sources(
        Some(&file),
        &[
            ("SUNNYCLOUD_SYNC_MAX_ATTEMPTS_PER_BLOCK", "0"),
            ("SUNNYCLOUD_SYNC_LONGPOLL_WAIT", "56s"),
        ],
    );

    let found = problems(config_of(&sources));

    let range = |min: &str, max: &str| Reason::OutOfRange {
        min: min.into(),
        max: max.into(),
    };
    assert_eq!(
        found,
        vec![
            KeyProblem::new("sync_max_attempts_per_block", range("1", "100")),
            KeyProblem::new("sync_longpoll_wait", range("1s", "55s")),
        ]
    );
}

#[test]
fn an_unknown_sync_variable_is_refused() {
    let file = ConfigFile::write("unknown", &format!("{{{BUILD_KEYS}}}"));
    let sources = sources(Some(&file), &[("SUNNYCLOUD_SYNC_RETRY_BAS", "2s")]);

    let found = problems(config_of(&sources));

    assert_eq!(
        found,
        vec![KeyProblem::new(
            "SUNNYCLOUD_SYNC_RETRY_BAS",
            Reason::Unknown
        )]
    );
}

#[test]
fn the_build_injected_keys_are_required_by_the_section() {
    let found = problems(config_of(&sources(None, &[])));

    let keys: Vec<&str> = found.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "sync_server_url",
            "sync_client_family",
            "sync_client_version"
        ]
    );
    assert!(found.iter().all(|p| p.reason == Reason::MissingRequired));
}

#[test]
fn short_durations_are_accepted_outside_production() {
    let file = ConfigFile::write("short", &format!("{{{BUILD_KEYS}}}"));
    let mut sources = sources(
        Some(&file),
        &[
            ("SUNNYCLOUD_SYNC_RETRY_BASE", "5ms"),
            ("SUNNYCLOUD_SYNC_RETRY_MAX", "50ms"),
        ],
    );
    sources.environment = Environment::Test;

    let config = config_of(&sources).expect("test environment has no duration floors");

    assert_eq!(config.retry_policy().base, Duration::from_millis(5));
    assert_eq!(config.retry_policy().max, Duration::from_millis(50));
    assert_eq!(config.retry_policy().max_attempts_per_block, 8);
}

#[test]
fn reload_swaps_the_section_and_a_bad_document_keeps_the_previous_one() {
    let file = ConfigFile::write("reload", &format!("{{{BUILD_KEYS}}}"));
    let handle = ClientSettingsHandle::<ClientSettings>::new(sources(Some(&file), &[]))
        .expect("initial load");

    let reloaded = handle
        .reload_client_settings(&format!("{{{BUILD_KEYS}, \"sync_retry_base\": \"3s\"}}"))
        .expect("valid document");
    let rejected =
        handle.reload_client_settings(&format!("{{{BUILD_KEYS}, \"sync_retry_base\": \"2h\"}}"));

    assert_eq!(
        SyncConfig::from_settings(&reloaded)
            .expect("section")
            .retry_base,
        Duration::from_secs(3)
    );
    assert!(matches!(rejected, Err(ConfigError::Invalid(_))));
    assert_eq!(
        SyncConfig::from_settings(&handle.current())
            .expect("section")
            .retry_base,
        Duration::from_secs(3)
    );
}
