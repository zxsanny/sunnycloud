//! AC-6 and AC-7 of AZ-3966: a desktop device token is never refreshed, and the sign-in calls (device grant,
//! mobile code exchange, sign-out) return their typed results.

mod auth_support;
mod connect_support;
mod request_support;
mod support;

use std::time::{Duration, Instant};

use auth_support::{FakeAuth, SpyCredentials, connected_as, requests_to, root_view};
use request_support::clock;
use sc_error::ErrorCode;
use serde_json::{Value, json};
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{DeviceAuthorization, DevicePoll, SessionTokens};

fn body(request: &support::LoggedRequest) -> Value {
    serde_json::from_slice(&request.body).expect("the client sent a JSON body")
}

fn tokens(access: &str) -> SessionTokens {
    SessionTokens {
        access: access.to_owned(),
        refresh: None,
        expires_in: 3600,
    }
}

#[tokio::test]
async fn a_device_token_poll_answered_401_is_unauthorized_within_a_second_with_no_refresh() {
    // Arrange
    let credentials = SpyCredentials::device();
    let script = Script::new().always(
        Route::DeviceTokens,
        Answer::problem(ErrorCode::Unauthenticated),
    );
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    let started = Instant::now();

    // Act
    let polled = api.device_grant_poll("device-code").await;

    // Assert
    assert_eq!(polled, Err(SyncError::Unauthorized));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(requests_to(&fixture, Route::RefreshTokens).is_empty());
    assert!(credentials.persisted().is_empty());
}

#[tokio::test]
async fn a_device_token_call_answered_401_is_unauthorized_at_once_with_no_refresh_and_then_silence()
{
    // Arrange
    let credentials = SpyCredentials::device();
    let script = Script::new().always(Route::RootNode, Answer::problem(ErrorCode::Unauthenticated));
    let (api, fixture) = connected_as(script, credentials.clone()).await;
    let started = Instant::now();

    // Act
    let first = api.root_node().await;
    let elapsed = started.elapsed();
    let second = api.root_node().await;

    // Assert
    assert_eq!(first, Err(SyncError::Unauthorized));
    assert_eq!(second, Err(SyncError::Unauthorized));
    assert!(elapsed < Duration::from_secs(1));
    assert_eq!(requests_to(&fixture, Route::RootNode).len(), 1);
    assert!(requests_to(&fixture, Route::RefreshTokens).is_empty());
    assert!(credentials.persisted().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_sign_in_ends_the_stop_after_a_revoked_session() {
    // Arrange
    let auth = FakeAuth::new(clock(), Duration::ZERO);
    let credentials = SpyCredentials::session(Duration::ZERO);
    auth.revoke();
    let exchange = {
        let auth = auth.clone();
        move |_: &support::LoggedRequest| Answer::json(200, &auth.mint_tokens())
    };
    let script = auth
        .wire(Script::new(), &[(Route::RootNode, root_view())])
        .respond(Route::MobileCodeExchange, exchange);
    let (api, _fixture) = connected_as(script, credentials.clone()).await;
    auth.expire_access();
    let revoked = api.root_node().await;

    // Act
    let signed_in = api
        .exchange_mobile_code("one-time", "verifier")
        .await
        .expect("the fake mints a pair");
    let refresh = signed_in
        .refresh
        .clone()
        .expect("a session has a refresh token");
    credentials.store(&signed_in.access, &refresh);
    let after = api.root_node().await;

    // Assert
    assert_eq!(revoked, Err(SyncError::Unauthorized));
    assert!(after.is_ok(), "{after:?}");
}

#[tokio::test]
async fn a_reused_mobile_code_is_invalid_and_a_good_one_returns_the_session_tokens() {
    // Arrange
    let granted = SessionTokens {
        access: "A".to_owned(),
        refresh: Some("R".to_owned()),
        expires_in: 900,
    };
    let script = Script::new()
        .on(Route::MobileCodeExchange, Answer::json(200, &granted))
        .on(
            Route::MobileCodeExchange,
            Answer::problem_with("invalid_code", 400, "Invalid code"),
        );
    let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;

    // Act
    let first = api.exchange_mobile_code("one-time", "verifier").await;
    let reused = api.exchange_mobile_code("one-time", "verifier").await;

    // Assert
    assert_eq!(first, Ok(granted));
    assert_eq!(reused, Err(SyncError::InvalidCode));
    let sent = requests_to(&fixture, Route::MobileCodeExchange);
    assert_eq!(sent[0].target, "/api/v1/auth/mobile-code-exchanges");
    assert_eq!(
        body(&sent[0]),
        json!({ "code": "one-time", "code_verifier": "verifier" })
    );
    assert_eq!(sent[0].header("authorization"), None);
}

#[tokio::test]
async fn device_grant_polls_answer_pending_slow_down_denied_expired_and_a_token_in_order() {
    // Arrange
    let authorization = DeviceAuthorization {
        device_code: "dc".to_owned(),
        user_code: "ABCD-1234".to_owned(),
        verification_uri: "https://cloud.example/device".to_owned(),
        interval: 5,
    };
    let script = [
        DevicePoll::Pending,
        DevicePoll::SlowDown,
        DevicePoll::Denied,
        DevicePoll::Expired,
        DevicePoll::Token(tokens("device-token")),
    ]
    .iter()
    .fold(
        Script::new().on(Route::DeviceGrants, Answer::json(200, &authorization)),
        |script, poll| script.on(Route::DeviceTokens, Answer::json(200, poll)),
    );
    let (api, fixture) = connected_as(script, SpyCredentials::device()).await;

    // Act
    let requested = api.device_grant_request("Work laptop", "linux").await;
    let mut polls = Vec::new();
    for _ in 0..5 {
        polls.push(api.device_grant_poll("dc").await);
    }

    // Assert
    assert_eq!(requested, Ok(authorization));
    assert_eq!(
        polls,
        vec![
            Ok(DevicePoll::Pending),
            Ok(DevicePoll::SlowDown),
            Ok(DevicePoll::Denied),
            Ok(DevicePoll::Expired),
            Ok(DevicePoll::Token(tokens("device-token"))),
        ]
    );
    let grant = requests_to(&fixture, Route::DeviceGrants);
    assert_eq!(
        body(&grant[0]),
        json!({ "device_name": "Work laptop", "platform": "linux" })
    );
    let polled = requests_to(&fixture, Route::DeviceTokens);
    assert_eq!(polled.len(), 5);
    assert_eq!(body(&polled[0]), json!({ "device_code": "dc" }));
    assert!(polled.iter().all(|p| p.header("authorization").is_none()));
}

#[tokio::test]
async fn sign_out_revokes_a_device_token_through_the_device_route() {
    // Arrange
    let script = Script::new().always(Route::SignOut, Answer::no_content());
    let (api, fixture) = connected_as(script, SpyCredentials::device()).await;

    // Act
    let result = api.sign_out().await;

    // Assert
    assert_eq!(result, Ok(()));
    let sent = requests_to(&fixture, Route::SignOut);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer ACC-SENTINEL-0")
    );
}

#[tokio::test]
async fn sign_out_revokes_a_session_through_the_session_route() {
    // Arrange
    let script = Script::new().always(Route::SignOutSession, Answer::no_content());
    let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;

    // Act
    let result = api.sign_out().await;

    // Assert
    assert_eq!(result, Ok(()));
    assert_eq!(requests_to(&fixture, Route::SignOutSession).len(), 1);
    assert!(requests_to(&fixture, Route::SignOut).is_empty());
}
