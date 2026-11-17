//! AZ-3797: `key-check`, `admin keycheck-init` and `admin rewrap` through the real handler with a scripted
//! `MasterKeyring`.

mod config_support;
mod key_support;
mod support;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use contracts::{KeyError, MasterKeyring};
use key_support::{Scripted, as_service};
use sc_config::{ConfigSources, load_keys};
use serde_json::Value;
use sunnycloud_server::{CliError, CliSettings, ConfigCheck, ServeSettings};
use support::{Captured, Harness, locked};
use uuid::Uuid;

type Refusal = fn() -> KeyError;

const PROGRESS: Duration = Duration::from_millis(500);

fn harness(keyring: &Arc<Scripted>) -> Harness {
    let service = as_service(keyring);
    Harness::new()
        .real_handler()
        .settings(CliSettings {
            progress_interval: PROGRESS,
            ..CliSettings::default()
        })
        .service::<dyn MasterKeyring, _, _>(move |_| {
            let service = Arc::clone(&service);
            async move { Ok(service) }
        })
}

fn run(keyring: &Arc<Scripted>, args: &[&str]) -> Captured {
    harness(keyring).run(args)
}

/// A `ServeSettings` whose `master_key_file` is a file holding `key`.
fn with_key_file(harness: Harness, key: &[u8]) -> Harness {
    let mut setup = config_support::Setup::valid(0);
    let path = setup.dir.join("master.key");
    fs::write(&path, key).expect("key file");
    setup.secret("master_key_file", &path.display().to_string());
    with_settings(harness, setup)
}

/// A `ServeSettings` loaded from `setup`, which stays alive (and its files with it) as long as the harness.
fn with_settings(harness: Harness, setup: config_support::Setup) -> Harness {
    let sources = ConfigSources {
        env: setup.env.clone().into_iter().collect(),
        ..ConfigSources::default()
    };
    let setup = Arc::new(setup);
    harness.service::<ServeSettings, _, _>(move |_| {
        let _alive = Arc::clone(&setup);
        let loaded = load_keys::<ServeSettings>(&sources, &ConfigCheck::Server.keys())
            .map_err(CliError::from);
        async move { loaded }
    })
}

fn json(result: &Captured) -> Value {
    serde_json::from_str(&result.stdout).expect("one JSON object")
}

#[test]
fn key_check_exits_0_on_a_match_and_2_with_the_code_for_every_refusal() {
    // Arrange
    let refusals: [(Refusal, &str); 4] = [
        (|| KeyError::FileMissing, "FileMissing"),
        (|| KeyError::WrongLength, "WrongLength"),
        (|| KeyError::KeyCheckMismatch, "KeyCheckMismatch"),
        (
            || KeyError::KeyCheckAbsentButBlobsExist,
            "KeyCheckAbsentButBlobsExist",
        ),
    ];

    // Act
    let matched = run(&Arc::new(Scripted::new()), &["key-check"]);
    let matched_json = run(&Arc::new(Scripted::new()), &["--json", "key-check"]);

    // Assert
    assert_eq!(matched.exit, 0, "{}", matched.stderr);
    assert!(
        matched.stdout.contains("current_key_id: 2"),
        "{}",
        matched.stdout
    );
    assert_eq!(
        json(&matched_json)["data"]["key_ids"],
        serde_json::json!([1, 2])
    );
    for (error, code) in refusals {
        let text = run(
            &Arc::new(Scripted::new().verify(move || Err(error()))),
            &["key-check"],
        );
        let object = run(
            &Arc::new(Scripted::new().verify(move || Err(error()))),
            &["--json", "key-check"],
        );
        assert_eq!(text.exit, 2, "{code}");
        assert_eq!(object.exit, 2, "{code}");
        assert_eq!(json(&object)["code"], code);
    }
}

#[test]
fn keycheck_init_passes_the_configured_key_file_and_maps_the_outcomes() {
    // Arrange
    let key = [0x42u8; 32];
    let good = Arc::new(Scripted::new());
    let bad = Arc::new(Scripted::new().init(|| {
        Err(KeyError::KeyCheckSampleFailed {
            blob_id: Uuid::from_u128(1),
        })
    }));

    // Act
    let ok = with_key_file(harness(&good), &key).run(&["admin", "keycheck-init"]);
    let refused = with_key_file(harness(&bad), &key).run(&["--json", "admin", "keycheck-init"]);

    // Assert
    assert_eq!(ok.exit, 0, "{}", ok.stderr);
    assert_eq!(*locked(&good.candidates), vec![key.to_vec()]);
    assert_eq!(refused.exit, 2);
    assert_eq!(json(&refused)["code"], "KeyCheckSampleFailed");
}

#[test]
fn keycheck_init_with_a_missing_key_file_exits_2_without_calling_the_keyring() {
    // Arrange
    let keyring = Arc::new(Scripted::new());
    let mut setup = config_support::Setup::valid(0);
    let absent = setup.dir.join("absent.key");
    setup.secret("master_key_file", &absent.display().to_string());
    let harness = with_settings(harness(&keyring), setup);

    // Act
    let result = harness.run(&["--json", "admin", "keycheck-init"]);

    // Assert
    assert_eq!(result.exit, 2, "{}", result.stderr);
    assert_eq!(json(&result)["code"], "FileMissing");
    assert!(locked(&keyring.candidates).is_empty());
}

#[test]
fn rewrap_without_yes_on_a_non_tty_exits_3_and_calls_nothing() {
    // Arrange
    let keyring = Arc::new(Scripted::new().batches(&[5]));

    // Act
    let result = run(&keyring, &["admin", "rewrap", "--from-key-id", "1"]);

    // Assert
    assert_eq!(result.exit, 3);
    assert!(locked(&keyring.batch_calls).is_empty());
    assert_eq!(*locked(&keyring.verifies), 0);
}

#[test]
fn rewrap_loops_to_zero_with_progress_and_a_keep_the_old_key_message() {
    // Arrange: 200 + 200 + 100 rows, then 0; every call takes longer than the 500 ms progress interval.
    let keyring = Arc::new(
        Scripted::new()
            .batches(&[200, 200, 100])
            .delay(Duration::from_millis(1200)),
    );

    // Act
    let result = run(
        &keyring,
        &["--json", "admin", "rewrap", "--from-key-id", "1", "--yes"],
    );

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert_eq!(
        *locked(&keyring.batch_calls),
        vec![(1, 2, 100); 4],
        "four calls, from the old key to the current one"
    );
    let progress = result
        .stderr
        .lines()
        .filter(|line| line.starts_with("rewrap: "))
        .count();
    assert!(
        progress >= 8,
        "{progress} progress lines: {}",
        result.stderr
    );
    let data = &json(&result)["data"];
    assert_eq!(data["rewrapped"], 500);
    assert!(data["note"].as_str().expect("note").contains("keep key 1"));
}

#[test]
fn rewrap_refuses_a_from_key_that_is_not_an_old_ring_key_before_any_batch() {
    // Arrange
    let keyring = Arc::new(Scripted::new());

    // Act
    let unknown = run(
        &keyring,
        &["admin", "rewrap", "--from-key-id", "9", "--yes"],
    );
    let current = run(
        &keyring,
        &["admin", "rewrap", "--from-key-id", "2", "--yes"],
    );

    // Assert
    assert_eq!(unknown.exit, 3, "{}", unknown.stderr);
    assert_eq!(current.exit, 3, "{}", current.stderr);
    assert!(locked(&keyring.batch_calls).is_empty());
}

#[test]
fn a_keyring_refusal_during_rewrap_exits_2_with_its_code() {
    // Arrange
    let keyring = Arc::new(Scripted::new().verify(|| Err(KeyError::KeyCheckMismatch)));

    // Act
    let result = run(
        &keyring,
        &["--json", "admin", "rewrap", "--from-key-id", "1", "--yes"],
    );

    // Assert
    assert_eq!(result.exit, 2);
    assert_eq!(json(&result)["code"], "KeyCheckMismatch");
}

#[test]
fn no_key_material_reaches_any_output_or_the_log() {
    // Arrange: the key file holds a known key; hex and base64 of it must never appear.
    let key = *b"0123456789abcdef0123456789abcdef";
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    let keyring = Arc::new(Scripted::new().batches(&[3]));

    // Act
    let outputs = [
        run(&keyring, &["--json", "key-check"]),
        with_key_file(harness(&keyring).log_json(), &key).run(&[
            "--json",
            "admin",
            "keycheck-init",
        ]),
        harness(&keyring).log_json().run(&[
            "--json",
            "admin",
            "rewrap",
            "--from-key-id",
            "1",
            "--yes",
        ]),
    ];

    // Assert
    for output in &outputs {
        let all = format!(
            "{}{}{}",
            output.stdout,
            output.stderr,
            output.log.join("\n")
        );
        assert!(!all.contains(&hex), "{all}");
        assert!(
            !all.contains("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY"),
            "{all}"
        );
        assert!(!all.contains("0123456789abcdef"), "{all}");
    }
}
