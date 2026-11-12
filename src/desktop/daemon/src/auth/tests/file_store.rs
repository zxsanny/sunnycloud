//! AC-1 of AZ-3908 for the Linux file fallback (IT-14-18 file cases).

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use sc_config::Secret;
use tempfile::TempDir;

use super::{ACCOUNT, DEVICE_TOKEN};
use crate::auth::token_store::{TokenStore, TokenStoreError, UserFileTokenStore};

fn chmod(path: &std::path::Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

fn token() -> Secret<String> {
    Secret::new(DEVICE_TOKEN.to_owned())
}

#[test]
fn save_writes_a_0600_file_in_a_0700_dir_and_leaves_no_temp_file() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());

    // Act
    store.save(ACCOUNT, &token()).expect("save");

    // Assert
    let file = state
        .path()
        .join("sunnycloud")
        .join(format!("token-{ACCOUNT}"));
    assert_eq!(store.path_for(ACCOUNT), file);
    assert_eq!(fs::metadata(&file).expect("file").mode() & 0o777, 0o600);
    let dir_mode = fs::metadata(state.path().join("sunnycloud"))
        .expect("dir")
        .mode();
    assert_eq!(dir_mode & 0o777, 0o700);
    assert_eq!(
        fs::read_dir(state.path().join("sunnycloud"))
            .expect("dir")
            .count(),
        1
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), DEVICE_TOKEN);
}

#[test]
fn load_returns_the_saved_token_and_none_after_delete() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());
    store.save(ACCOUNT, &token()).expect("save");

    // Act
    let loaded = store.load(ACCOUNT).expect("load");
    store.delete(ACCOUNT).expect("delete");
    let after = store.load(ACCOUNT).expect("load after delete");

    // Assert
    assert_eq!(
        loaded.map(|t| t.expose().clone()).as_deref(),
        Some(DEVICE_TOKEN)
    );
    assert!(after.is_none());
    assert_eq!(store.delete(ACCOUNT), Ok(()));
}

#[test]
fn a_0644_token_file_is_permission_too_open() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());
    store.save(ACCOUNT, &token()).expect("save");
    chmod(&store.path_for(ACCOUNT), 0o644);

    // Act
    let loaded = store.load(ACCOUNT);

    // Assert
    assert_eq!(loaded.err(), Some(TokenStoreError::PermissionTooOpen));
}

#[test]
fn a_0755_token_dir_is_permission_too_open_for_load_and_save() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());
    store.save(ACCOUNT, &token()).expect("save");
    chmod(&state.path().join("sunnycloud"), 0o755);

    // Act
    let loaded = store.load(ACCOUNT);
    let saved = store.save(ACCOUNT, &token());

    // Assert
    assert_eq!(loaded.err(), Some(TokenStoreError::PermissionTooOpen));
    assert_eq!(saved, Err(TokenStoreError::PermissionTooOpen));
}

#[test]
fn a_symlinked_token_file_is_permission_too_open() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());
    store.save(ACCOUNT, &token()).expect("save");
    let elsewhere = state.path().join("elsewhere");
    fs::rename(store.path_for(ACCOUNT), &elsewhere).expect("move");
    std::os::unix::fs::symlink(&elsewhere, store.path_for(ACCOUNT)).expect("symlink");

    // Act
    let loaded = store.load(ACCOUNT);

    // Assert
    assert_eq!(loaded.err(), Some(TokenStoreError::PermissionTooOpen));
}

#[test]
fn load_without_a_token_dir_is_none() {
    // Arrange
    let state = TempDir::new().expect("state dir");
    let store = UserFileTokenStore::new(state.path());

    // Act
    let loaded = store.load(ACCOUNT).expect("load");

    // Assert
    assert!(loaded.is_none());
}
