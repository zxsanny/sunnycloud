//! IT-14-18 and FT-P-61 on this Linux host: the real keyring 4.2.0 and the real file fallback, each context in its
//! own isolated process (see `token_store_support`). AC-1 and AC-2 of AZ-3908.
#![cfg(target_os = "linux")]

mod token_store_support;

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_config::Secret;
use sunnycloud_daemon::auth::token_store::{
    FILE_FALLBACK_WARNING, StoreLimits, SystemTokenStore, TokenStore, TokenStoreError,
};
use token_store_support::{
    ACCOUNT, Bus, CHILD_ENV, Captured, StateHome, assert_not_in_process, run_child,
    token_from_stdin,
};
use uuid::Uuid;

const TOKEN: &str = "fake-device-token-3f9c1e7a5b";

fn assert_child(case: &str, bus: Bus, state_home: StateHome) {
    // Act
    let run = run_child(case, bus, TOKEN, state_home);

    // Assert
    assert!(run.ok, "child {case} failed:\n{}", run.output);
    assert!(
        run.output.contains("1 passed"),
        "child {case} did not run:\n{}",
        run.output
    );
}

#[test]
fn unlocked_secret_service_holds_one_keyring_entry_and_no_file() {
    assert_child(
        "child_keyring_unlocked",
        Bus::KeyringUnlocked,
        StateHome::Set,
    );
}

#[test]
fn no_session_bus_uses_the_default_state_dir_file_with_one_warning() {
    assert_child("child_file_fallback", Bus::None, StateHome::Unset);
}

#[test]
fn a_bus_without_secret_service_uses_the_xdg_state_home_file_with_one_warning() {
    assert_child(
        "child_file_fallback",
        Bus::WithoutSecretService,
        StateHome::Set,
    );
}

#[test]
fn a_never_unlocked_keyring_is_store_unavailable_without_a_file() {
    assert_child(
        "child_keyring_unusable",
        Bus::KeyringNeverUnlocked,
        StateHome::Set,
    );
}

#[test]
fn a_locked_keyring_is_store_unavailable_within_a_second_without_a_file() {
    assert_child("child_keyring_unusable", Bus::KeyringLocked, StateHome::Set);
}

#[test]
fn an_unanswered_unlock_prompt_is_store_unavailable_after_the_bound() {
    assert_child(
        "child_prompt_unanswered",
        Bus::KeyringPromptUnanswered,
        StateHome::Set,
    );
}

// ---- children: each runs only inside its isolated process ----

fn in_child() -> bool {
    let inside = std::env::var_os(CHILD_ENV).is_some();
    if !inside {
        println!("runs only as an isolated child of the IT-14-18 tests");
    }
    inside
}

fn account() -> Uuid {
    Uuid::parse_str(ACCOUNT).expect("account id")
}

fn expected_file() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".local/state")
        });
    state.join("sunnycloud").join(format!("token-{ACCOUNT}"))
}

fn secret_tool(args: &[&str]) -> String {
    let output = Command::new("secret-tool")
        .args(args)
        .output()
        .expect("run secret-tool");
    String::from_utf8(output.stdout).expect("utf-8")
}

#[test]
#[ignore = "isolated child of unlocked_secret_service_holds_one_keyring_entry_and_no_file"]
fn child_keyring_unlocked() {
    if !in_child() {
        return;
    }
    // Arrange
    let token = token_from_stdin();
    let captured = Captured::default();
    let _installed = captured.install();
    let store = SystemTokenStore::new(StoreLimits::default());

    // Act
    store
        .save(account(), &Secret::new(token.clone()))
        .expect("save to the keyring");
    let loaded = store.load(account()).expect("load from the keyring");

    // Assert
    assert_eq!(loaded.map(|t| t.expose().clone()), Some(token.clone()));
    let lookup = secret_tool(&["lookup", "service", "sunnycloud", "username", ACCOUNT]);
    assert_eq!(lookup, token);
    let search = secret_tool(&["search", "--all", "service", "sunnycloud"]);
    assert_eq!(search.lines().filter(|l| l.starts_with('[')).count(), 1);
    assert!(!expected_file().exists());
    assert!(captured.warnings().is_empty(), "{:?}", captured.warnings());
    assert!(
        captured
            .events()
            .iter()
            .all(|(_, text)| !text.contains(&token))
    );
    assert_not_in_process(&token);
    store.delete(account()).expect("delete");
    assert!(store.load(account()).expect("load after delete").is_none());
}

#[test]
#[ignore = "isolated child of the file fallback tests"]
fn child_file_fallback() {
    if !in_child() {
        return;
    }
    // Arrange
    let token = token_from_stdin();
    let captured = Captured::default();
    let _installed = captured.install();
    let store = SystemTokenStore::new(StoreLimits::default());
    let started = Instant::now();

    // Act
    store
        .save(account(), &Secret::new(token.clone()))
        .expect("save to the file");
    let decided_in = started.elapsed();
    let loaded = store.load(account()).expect("load from the file");
    store
        .save(account(), &Secret::new(token.clone()))
        .expect("save again");

    // Assert
    assert!(decided_in < Duration::from_secs(5), "{decided_in:?}");
    assert_eq!(loaded.map(|t| t.expose().clone()), Some(token.clone()));
    let file = expected_file();
    let file_meta = std::fs::metadata(&file).expect("token file");
    let dir_meta = std::fs::metadata(file.parent().expect("dir")).expect("token dir");
    assert_eq!(file_meta.mode() & 0o777, 0o600);
    assert_eq!(dir_meta.mode() & 0o777, 0o700);
    assert_eq!(file_meta.uid(), own_uid());
    let warnings = captured.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains(&format!("message={FILE_FALLBACK_WARNING}")));
    assert!(warnings[0].contains("component=\"token_store\""));
    assert!(warnings[0].contains(&format!("path={}", file.display())));
    assert!(
        captured
            .events()
            .iter()
            .all(|(_, text)| !text.contains(&token))
    );
    assert_not_in_process(&token);
}

fn own_uid() -> u32 {
    std::fs::metadata(std::env::var_os("HOME").expect("HOME"))
        .expect("home")
        .uid()
}

#[test]
#[ignore = "isolated child of the unusable keyring tests"]
fn child_keyring_unusable() {
    if !in_child() {
        return;
    }
    // Arrange
    let token = token_from_stdin();
    let captured = Captured::default();
    let _installed = captured.install();
    let store = SystemTokenStore::new(StoreLimits::default());
    let started = Instant::now();

    // Act
    let saved = store.save(account(), &Secret::new(token.clone()));
    let took = started.elapsed();

    // Assert
    assert_eq!(saved, Err(TokenStoreError::StoreUnavailable));
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert!(!expected_file().exists());
    assert!(
        captured
            .warnings()
            .iter()
            .all(|w| !w.contains(FILE_FALLBACK_WARNING))
    );
    assert!(
        captured
            .events()
            .iter()
            .all(|(_, text)| !text.contains(&token))
    );
}

#[tokio::test]
#[ignore = "isolated child of an_unanswered_unlock_prompt_is_store_unavailable_after_the_bound"]
async fn child_prompt_unanswered() {
    if !in_child() {
        return;
    }
    // Arrange
    let token = Secret::new(token_from_stdin());
    let bound = Duration::from_millis(1500);
    let limits = StoreLimits {
        detect_bound: Duration::from_secs(5),
        call_bound: bound,
    };
    let store = Arc::new(SystemTokenStore::new(limits));
    let started = Instant::now();

    // Act
    let saving = {
        let store = Arc::clone(&store);
        tokio::task::spawn_blocking(move || store.save(account(), &token))
    };
    let (ping, pong) = tokio::sync::oneshot::channel();
    tokio::spawn(async move { ping.send(()).expect("pong receiver") });
    let answered = tokio::time::timeout(Duration::from_millis(500), pong).await;
    let still_waiting = !saving.is_finished();
    let saved = saving.await.expect("save task");
    let took = started.elapsed();

    // Assert
    assert!(
        answered.is_ok(),
        "the runtime did not answer while the keyring call hung"
    );
    assert!(still_waiting);
    assert_eq!(saved, Err(TokenStoreError::StoreUnavailable));
    assert!(took >= bound, "{took:?}");
    assert!(took < bound + Duration::from_secs(2), "{took:?}");
    assert!(!expected_file().exists());
}
