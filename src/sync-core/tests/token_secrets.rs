//! AC-9 of AZ-3966: over every failure path of the token lifecycle no token reaches a request URL, the `Display`
//! or `Debug` of a result, or a log line (DEBUG and up).

mod auth_support;
mod connect_support;
mod request_support;
mod support;

use std::fmt::Debug;
use std::time::Duration;

use auth_support::{FakeAuth, SpyCredentials, connected_as, root_view};
use request_support::{capture_logs, clock};
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, Fixture, Route, Script};
use sync_core::{SyncApi, SyncError};

const SENTINELS: [&str; 2] = ["ACC-SENTINEL", "REF-SENTINEL"];

/// `Debug` of a result, and `Display` of its error.
fn show<T: Debug>(result: &Result<T, SyncError>) -> String {
    match result {
        Ok(_) => format!("{result:?}"),
        Err(error) => format!("{result:?} {error}"),
    }
}

fn urls(fixture: &Fixture) -> String {
    fixture
        .requests()
        .iter()
        .map(|request| format!("{}\n", request.target))
        .collect()
}

#[tokio::test]
async fn no_failure_path_of_the_token_lifecycle_leaks_a_token() {
    // Arrange
    let (logs, _guard) = capture_logs();
    let mut visible = String::new();
    let leaky = json!({
        "access": "ACC-SENTINEL-9",
        "refresh": "REF-SENTINEL-9",
        "expires_in": "REF-SENTINEL-9",
    });

    // Act
    // 1. the host cannot store the rotated pair
    {
        let auth = FakeAuth::new(clock(), Duration::ZERO);
        let credentials = SpyCredentials::session(Duration::ZERO);
        credentials.fail_persists(2);
        let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
        let (api, fixture) = connected_as(script, credentials).await;
        auth.expire_access();
        let result = api.root_node().await;
        assert_eq!(result, Err(SyncError::Unauthorized));
        visible += &show(&result);
        visible += &urls(&fixture);
    }
    // 2. the session is revoked
    {
        let auth = FakeAuth::new(clock(), Duration::ZERO);
        auth.revoke();
        let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
        let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;
        auth.expire_access();
        let result = api.root_node().await;
        let refreshed = api.refresh().await;
        assert_eq!(result, Err(SyncError::Unauthorized));
        visible += &(show(&result) + &show(&refreshed));
        visible += &urls(&fixture);
    }
    // 3. a device token is refused
    {
        let script = Script::new()
            .always(Route::RootNode, Answer::problem(ErrorCode::Unauthenticated))
            .always(
                Route::DeviceTokens,
                Answer::problem(ErrorCode::Unauthenticated),
            );
        let (api, fixture) = connected_as(script, SpyCredentials::device()).await;
        visible += &show(&api.root_node().await);
        visible += &show(&api.device_grant_poll("dc").await);
        visible += &urls(&fixture);
    }
    // 4. the refresh response is lost once and the retry succeeds: the result and its Debug stay clean
    {
        let auth = FakeAuth::new(clock(), Duration::ZERO);
        auth.lose_next_refresh_response();
        let script = auth.wire(Script::new(), &[(Route::RootNode, root_view())]);
        let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;
        auth.expire_access();
        let result = api.root_node().await;
        assert!(result.is_ok(), "{result:?}");
        visible += &show(&result);
        visible += &urls(&fixture);
    }
    // 5. the refresh answer is malformed and quotes both tokens
    {
        let script = Script::new()
            .always(Route::RootNode, Answer::problem(ErrorCode::Unauthenticated))
            .always(Route::RefreshTokens, Answer::json(200, &leaky));
        let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;
        let result = api.root_node().await;
        assert!(
            matches!(result, Err(SyncError::ProtocolViolation { .. })),
            "{result:?}"
        );
        visible += &format!(
            "{} {}",
            result.as_ref().expect_err("a violation"),
            show(&result)
        );
        visible += &urls(&fixture);
    }
    // 6. a reused code, and a malformed exchange answer that quotes the tokens
    {
        let script = Script::new()
            .on(
                Route::MobileCodeExchange,
                Answer::problem_with("invalid_code", 400, "Invalid code"),
            )
            .on(Route::MobileCodeExchange, Answer::json(200, &leaky));
        let (api, fixture) = connected_as(script, SpyCredentials::session(Duration::ZERO)).await;
        let reused = api
            .exchange_mobile_code("REF-SENTINEL-c", "ACC-SENTINEL-v")
            .await;
        let malformed = api.exchange_mobile_code("c", "v").await;
        assert_eq!(reused, Err(SyncError::InvalidCode));
        assert!(
            matches!(malformed, Err(SyncError::ProtocolViolation { .. })),
            "{malformed:?}"
        );
        visible += &(show(&reused) + &show(&malformed));
        visible += &urls(&fixture);
    }
    // 7. a granted token comes back in a result that is printed
    {
        let granted = json!({
            "access_token": "ACC-SENTINEL-t",
            "token_type": "Bearer",
        });
        let script = Script::new().always(Route::DeviceTokens, Answer::json(200, &granted));
        let (api, fixture) = connected_as(script, SpyCredentials::device()).await;
        let result = api.device_grant_poll("dc").await;
        assert!(
            matches!(result, Ok(sync_core_proto::DevicePoll::Token(_))),
            "{result:?}"
        );
        visible += &show(&result);
        visible += &urls(&fixture);
    }

    // Assert
    let log_text = logs.text();
    for sentinel in SENTINELS {
        assert_eq!(
            visible.matches(sentinel).count(),
            0,
            "{sentinel} in {visible}"
        );
        assert_eq!(
            log_text.matches(sentinel).count(),
            0,
            "{sentinel} in {log_text}"
        );
    }
    assert!(log_text.contains("stopping work"), "{log_text}");
    assert!(log_text.contains("could not store"), "{log_text}");
    assert!(log_text.contains("does not match its DTO"), "{log_text}");
}
