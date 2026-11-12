//! AC-1 and AC-2 of AZ-4236: the mobile code exchange and the refresh answers are decoded in the server's shape
//! `{access_token, refresh_token, expires_in}`, the old shape is a protocol violation, and `FakeAuth` answers the
//! server's shape.

mod auth_support;
mod connect_support;
mod request_support;
mod support;

use std::time::Duration;

use auth_support::{FakeAuth, SpyCredentials, connected_as, root_view};
use request_support::clock;
use serde_json::json;
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::SessionTokens;

fn violation() -> SyncError {
    SyncError::ProtocolViolation {
        detail: "malformed response body",
    }
}

#[tokio::test]
async fn the_exchange_decodes_the_server_answer_and_refuses_the_old_shape() {
    // Arrange
    let server = json!({ "access_token": "A", "refresh_token": "R", "expires_in": 900 });
    let old = json!({ "access": "A", "refresh": "R", "expires_in": 900 });
    let script = Script::new()
        .on(Route::MobileCodeExchange, Answer::json(200, &server))
        .on(Route::MobileCodeExchange, Answer::json(200, &old));
    let (api, _fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;

    // Act
    let current = api.exchange_mobile_code("code", "verifier").await;
    let outdated = api.exchange_mobile_code("code", "verifier").await;

    // Assert
    assert_eq!(
        current,
        Ok(SessionTokens {
            access_token: "A".to_owned(),
            refresh_token: "R".to_owned(),
            expires_in: 900,
        })
    );
    assert_eq!(outdated, Err(violation()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refresh_answer_in_the_old_shape_is_a_protocol_violation_and_nothing_is_stored() {
    // Arrange
    let old = json!({ "access": "A", "refresh": "R", "expires_in": 900 });
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = Script::new().always(Route::RefreshTokens, Answer::json(200, &old));
    let (api, _fixture) = connected_as(script, credentials.clone()).await;

    // Act
    let refreshed = api.refresh().await;

    // Assert
    assert_eq!(refreshed, Err(violation()));
    assert!(credentials.persisted().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fake_auth_answers_the_server_shape_and_its_refresh_is_decoded() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
    let (api, _fixture) = connected_as(script, credentials.clone()).await;

    // Act
    let refreshed = api.refresh().await;
    let minted = auth.mint_tokens();

    // Assert
    let mut keys: Vec<&str> = minted
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["access_token", "expires_in", "refresh_token"]);
    assert!(minted["access_token"].is_string() && minted["refresh_token"].is_string());
    assert!(minted["expires_in"].is_u64());
    assert_eq!(refreshed, Ok(()));
    assert_eq!(credentials.persisted().len(), 1);
}
