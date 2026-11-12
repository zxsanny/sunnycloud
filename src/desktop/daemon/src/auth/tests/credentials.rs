//! The daemon's `CredentialProvider`: a device token read through the store for every request.

use std::sync::Arc;

use sc_config::Secret;
use sync_core::{CredentialProvider, PersistFailed};
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
    let first = credentials.bearer();
    store
        .save(ACCOUNT, &Secret::new("second-token".to_owned()))
        .expect("save");

    // Act
    let second = credentials.bearer();

    // Assert
    assert_eq!(first.access.expose(), "first-token");
    assert_eq!(second.access.expose(), "second-token");
    assert!(second.refresh.is_none());
    assert_eq!(format!("{second:?}"), "***");
}

#[test]
fn bearer_without_a_stored_token_is_empty() {
    // Arrange
    let (_state, _store, credentials) = provider();

    // Act
    let bearer = credentials.bearer();

    // Assert
    assert!(bearer.access.expose().is_empty());
    assert!(bearer.refresh.is_none());
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
