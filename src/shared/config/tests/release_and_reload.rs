mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use common::*;
use sc_config::{
    ClientSettings, ClientSettingsHandle, ConfigError, ConfigSources, Environment, Reader, Reason,
    load,
};

const SOURCE: &str = "https://updates.sunnycloud.example/manifest";

fn compiled() -> BTreeMap<String, String> {
    [
        ("updater_source", SOURCE),
        ("updater_public_key_1", "key-one"),
        ("updater_public_key_2", "key-two"),
        ("min_api_family", "1"),
        ("https_required", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

fn overridden_sources(dir: &TempDir) -> ConfigSources {
    let mut sources = valid_sources(Reader::Clients);
    for key in [
        "updater_source",
        "updater_public_key_1",
        "updater_public_key_2",
        "min_api_family",
        "https_required",
    ] {
        set_key(&mut sources, key, "false");
    }
    set_key(&mut sources, "updater_source", "https://evil.example/");
    sources.files.push(dir.write(
        "settings.json",
        r#"{"updater_source": "https://evil.example/file", "updater_public_key_1": "attacker"}"#,
    ));
    sources
}

#[test]
fn ac6_release_build_ignores_env_and_file_overrides() {
    let dir = TempDir::new();
    let mut sources = overridden_sources(&dir);
    sources.release_fixed = Some(compiled());

    let section = load::<ClientSettings>(&sources).expect("load");

    assert_eq!(section.updater_source.as_deref(), Some(SOURCE));
    assert_eq!(section.updater_public_key_1.as_deref(), Some("key-one"));
    assert_eq!(section.updater_public_key_2.as_deref(), Some("key-two"));
    assert_eq!(section.min_api_family.as_deref(), Some("1"));
    assert_eq!(section.https_required, Some(true));
}

#[test]
fn ac6_debug_build_still_takes_overrides() {
    let dir = TempDir::new();
    let sources = overridden_sources(&dir);

    let section = load::<ClientSettings>(&sources).expect("load");

    assert_eq!(
        section.updater_source.as_deref(),
        Some("https://evil.example/")
    );
}

#[test]
fn ac6_release_build_without_a_compiled_value_refuses_to_start() {
    let mut sources = valid_sources(Reader::Clients);
    let mut fixed = compiled();
    fixed.remove("updater_public_key_2");
    sources.release_fixed = Some(fixed);

    let problems = problems_of(load_reader(Reader::Clients, &sources));

    assert_eq!(
        problems,
        vec![problem("updater_public_key_2", Reason::MissingRequired)]
    );
}

#[test]
fn ac7_invalid_reload_keeps_the_previous_settings_and_valid_reload_replaces_them() {
    let mut sources = valid_sources(Reader::Clients);
    sources.environment = Environment::Production;
    let handle = ClientSettingsHandle::<ClientSettings>::new(sources).expect("initial");
    assert_eq!(handle.current().desktop_settle.get().as_secs(), 2);

    let rejected = handle.reload_client_settings(r#"{"desktop_settle": "banana"}"#);
    let after_rejected = handle.current();
    let accepted = handle.reload_client_settings(r#"{"desktop_settle": "5s"}"#);
    let after_accepted = handle.current();

    assert!(matches!(rejected, Err(ConfigError::Invalid(ref p)) if p.len() == 1));
    assert_eq!(after_rejected.desktop_settle.get().as_secs(), 2);
    assert_eq!(accepted.expect("valid").desktop_settle.get().as_secs(), 5);
    assert_eq!(after_accepted.desktop_settle.get().as_secs(), 5);
}

#[test]
fn ac7_below_floor_reload_is_rejected_in_production_and_malformed_json_is_invalid() {
    let handle = ClientSettingsHandle::<ClientSettings>::new(valid_sources(Reader::Clients))
        .expect("initial");

    for document in [r#"{"desktop_settle": "10ms"}"#, "{broken", r#"{"typo": 1}"#] {
        let result = handle.reload_client_settings(document);
        assert!(matches!(result, Err(ConfigError::Invalid(_))), "{document}");
    }
    assert_eq!(handle.current().desktop_settle.get().as_secs(), 2);
}

#[test]
fn ac7_reader_during_reloads_sees_old_or_new_never_a_mix() {
    let handle = Arc::new(
        ClientSettingsHandle::<ClientSettings>::new(valid_sources(Reader::Clients))
            .expect("initial"),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (handle, stop) = (Arc::clone(&handle), Arc::clone(&stop));
            thread::spawn(move || {
                let mut seen = 0_u32;
                while !stop.load(Ordering::Relaxed) {
                    let current = handle.current();
                    let pair = (
                        current.desktop_settle.get().as_secs(),
                        current.desktop_rescan_interval.get().as_secs(),
                    );
                    assert!(
                        matches!(pair, (2, 900) | (5, 60) | (7, 120)),
                        "mixed settings {pair:?}"
                    );
                    seen += 1;
                }
                seen
            })
        })
        .collect();

    for round in 0..200 {
        let document = if round % 2 == 0 {
            r#"{"desktop_settle": "5s", "desktop_rescan_interval": "1m"}"#
        } else {
            r#"{"desktop_settle": "7s", "desktop_rescan_interval": "2m"}"#
        };
        handle.reload_client_settings(document).expect("reload");
        assert!(
            handle
                .reload_client_settings(r#"{"desktop_settle": "x"}"#)
                .is_err()
        );
    }
    stop.store(true, Ordering::Relaxed);

    for reader in readers {
        assert!(reader.join().expect("reader panicked") > 0);
    }
}
