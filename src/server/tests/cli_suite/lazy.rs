//! AC-5 (IT-12-18): lazy construction. Each command reads only the keys and builds only the services it needs.

use std::process::{Command, Output, Stdio};

use super::config_support::Setup;
use sc_testdb::TestDb;

/// The binary with the given environment only: no provider, no worker token, nothing else.
fn run(setup: &Setup, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
        .args(args)
        .env_clear()
        .envs(std::env::var("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
        .envs(&setup.env)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn migrate_succeeds_with_only_the_migrator_url() {
    // Arrange
    let db = TestDb::clone_empty().await;
    let mut setup = Setup::empty();
    let url = db
        .config()
        .db_migrator_url
        .expect("a migrator URL")
        .expose()
        .clone();
    setup.secret("db_migrator_url", &url);

    // Act
    let out = tokio::task::spawn_blocking(move || run(&setup, &["migrate"]))
        .await
        .expect("the run");

    // Assert
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    db.drop_db().await;
}

/// The invite commands bind the database and identity keys only, and build 01's invites without sign-in.
#[tokio::test]
async fn setup_token_succeeds_with_no_provider_and_no_worker_token() {
    // Arrange
    let db = TestDb::clone_template().await;
    let mut setup = Setup::empty();
    setup.secret("db_api_url", db.config().db_api_url.expose());
    setup.set("PUBLIC_URL", "https://cloud.example.org");

    // Act
    let out = tokio::task::spawn_blocking(move || run(&setup, &["admin", "setup-token"]))
        .await
        .expect("the run");

    // Assert
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    db.drop_db().await;
}

/// The keyring commands bind the database and blobstore keys only.
#[tokio::test]
async fn key_check_succeeds_with_no_provider_and_no_worker_token() {
    // Arrange
    let db = TestDb::clone_template().await;
    let mut setup = Setup::empty();
    setup.secret("db_api_url", db.config().db_api_url.expose());
    let key_file = setup.dir.join("master.key");
    std::fs::write(&key_file, [7u8; 32]).expect("the key file is writable");
    setup.secret("master_key_file", &key_file.display().to_string());
    let blob_root = setup.dir.join("blobs");
    std::fs::create_dir_all(&blob_root).expect("the blob volume exists");
    setup.set("BLOB_ROOT", &blob_root.display().to_string());

    // Act
    let out = tokio::task::spawn_blocking(move || run(&setup, &["key-check"]))
        .await
        .expect("the run");

    // Assert
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    db.drop_db().await;
}

#[test]
fn openapi_succeeds_with_no_database_and_no_key() {
    // Arrange
    let setup = Setup::empty();

    // Act
    let out = run(&setup, &["openapi"]);

    // Assert
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}
