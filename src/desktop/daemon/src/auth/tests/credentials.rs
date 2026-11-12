//! The daemon's `CredentialProvider`: a device token read through the store for every request, and
//! `CredentialUnavailable` when the store has none or cannot be read (AZ-4231 AC-5).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use sc_config::Secret;
use sync_core::{CredentialProvider, CredentialUnavailable, PersistFailed};
use tempfile::TempDir;

use super::ACCOUNT;
use crate::auth::StoreCredentials;
use crate::auth::token_store::{TokenStore, UserFileTokenStore};

fn provider() -> (TempDir, Arc<UserFileTokenStore>, StoreCredentials) {
    let state = TempDir::new().expect("state dir");
    let store = Arc::new(UserFileTokenStore::new(state.path()));
    let credentials = StoreCredentials::new(Arc::clone(&store) as Arc<dyn TokenStore>, ACCOUNT);
    (state, store, credentials)
}

#[test]
fn bearer_reads_the_stored_token_on_every_request_with_no_refresh() {
    // Arrange
    let (_state, store, credentials) = provider();
    store
        .save(ACCOUNT, &Secret::new("first-token".to_owned()))
        .expect("save");
    let first = credentials.bearer().expect("a stored token");
    store
        .save(ACCOUNT, &Secret::new("second-token".to_owned()))
        .expect("save");

    // Act
    let second = credentials.bearer().expect("a stored token");

    // Assert
    assert_eq!(first.access.expose(), "first-token");
    assert_eq!(second.access.expose(), "second-token");
    assert!(second.refresh.is_none());
    assert_eq!(format!("{second:?}"), "***");
}

#[test]
fn bearer_without_a_stored_token_is_unavailable() {
    // Arrange
    let (_state, _store, credentials) = provider();

    // Act
    let bearer = credentials.bearer();

    // Assert
    assert_eq!(bearer.err(), Some(CredentialUnavailable));
}

#[test]
fn bearer_of_an_unreadable_store_is_unavailable_and_the_token_returns_when_it_recovers() {
    // Arrange
    let (_state, store, credentials) = provider();
    store
        .save(ACCOUNT, &Secret::new("kept-token".to_owned()))
        .expect("save");
    let file = store.path_for(ACCOUNT);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");

    // Act
    let unreadable = credentials.bearer();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("chmod");
    let recovered = credentials.bearer();

    // Assert
    assert_eq!(unreadable.err(), Some(CredentialUnavailable));
    assert_eq!(
        recovered.expect("the store recovered").access.expose(),
        "kept-token"
    );
}

#[test]
fn a_rotated_pair_is_refused() {
    // Arrange
    let (_state, store, credentials) = provider();

    // Act
    let rotated = credentials.on_rotated(
        Secret::new("access".to_owned()),
        Secret::new("refresh".to_owned()),
    );

    // Assert
    assert_eq!(rotated, Err(PersistFailed));
    assert!(store.load(ACCOUNT).expect("load").is_none());
}
