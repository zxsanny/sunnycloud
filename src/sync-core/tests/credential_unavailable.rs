//! AC-5 of AZ-4231: a host whose credential store cannot be read sends nothing, answers `CredentialUnavailable`,
//! never rotates or stops work for it, and the next call after the store recovers carries the token.

mod auth_support;
mod connect_support;
mod request_support;
mod support;

use std::time::Duration;

use auth_support::{SpyCredentials, connected_as, requests_to, root_view};
use sc_error::ErrorCode;
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};

#[tokio::test]
async fn an_unavailable_credential_sends_nothing_and_the_call_after_recovery_carries_the_token() {
    // Arrange
    let credentials = SpyCredentials::device();
    let script = Script::new().always(Route::RootNode, root_view());
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    credentials.set_unavailable(true);

    // Act
    let unavailable = api.root_node().await;
    let sent_while_unavailable = requests_to(&fixture, Route::RootNode).len();
    credentials.set_unavailable(false);
    let recovered = api.root_node().await;

    // Assert
    assert_eq!(unavailable, Err(SyncError::CredentialUnavailable));
    assert_eq!(sent_while_unavailable, 0);
    assert!(recovered.is_ok(), "{recovered:?}");
    let sent = requests_to(&fixture, Route::RootNode);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer ACC-SENTINEL-0")
    );
}

#[tokio::test]
async fn an_unavailable_credential_neither_refreshes_nor_stops_a_session() {
    // Arrange
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = Script::new().always(Route::RootNode, root_view());
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    credentials.set_unavailable(true);

    // Act
    let refreshed = api.refresh().await;
    let signed_out = api.sign_out().await;
    credentials.set_unavailable(false);
    let after = api.root_node().await;

    // Assert
    assert_eq!(refreshed, Err(SyncError::CredentialUnavailable));
    assert_eq!(signed_out, Err(SyncError::CredentialUnavailable));
    assert!(after.is_ok(), "{after:?}");
    assert!(requests_to(&fixture, Route::RefreshTokens).is_empty());
    assert!(requests_to(&fixture, Route::SignOut).is_empty());
    assert!(requests_to(&fixture, Route::SignOutSession).is_empty());
    assert!(credentials.persisted().is_empty());
}

#[tokio::test]
async fn a_401_to_a_request_that_carried_a_token_still_stops_work_after_an_unavailable_spell() {
    // Arrange
    let credentials = SpyCredentials::device();
    let script = Script::new().always(Route::RootNode, Answer::problem(ErrorCode::Unauthenticated));
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    credentials.set_unavailable(true);
    let unavailable = api.root_node().await;
    credentials.set_unavailable(false);

    // Act
    let refused = api.root_node().await;
    let after = api.root_node().await;

    // Assert
    assert_eq!(unavailable, Err(SyncError::CredentialUnavailable));
    assert_eq!(refused, Err(SyncError::Unauthorized));
    assert_eq!(after, Err(SyncError::Unauthorized));
    let sent = requests_to(&fixture, Route::RootNode);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer ACC-SENTINEL-0")
    );
}
