use std::time::Duration;

use contracts::{DeviceAuthorization, DeviceGrantError, SessionTokens, SignInError, SignInOutcome};
use serde_json::json;

use super::support::*;

fn limited() -> Duration {
    Duration::from_secs(30)
}

fn assert_limited(answer: &crate::worker::support::Reply) {
    assert_eq!(answer.status, 429);
    assert_eq!(answer.header("retry-after"), Some("30"));
    assert_eq!(answer.code(), "rate_limited");
    assert_eq!(answer.json()["retry_after"], 30);
}

#[tokio::test]
async fn begin_complete_the_grant_start_and_the_code_exchange_answer_429_with_retry_after() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SignInService::begin",
        Err::<contracts::AuthorizationRedirect, SignInError>(SignInError::RateLimited {
            retry_after: limited(),
        }),
    );
    stack.script.push(
        "SignInService::complete",
        Err::<SignInOutcome, SignInError>(SignInError::RateLimited {
            retry_after: limited(),
        }),
    );
    stack.script.push(
        "DeviceGrantService::request",
        Err::<DeviceAuthorization, DeviceGrantError>(DeviceGrantError::RateLimited {
            retry_after: limited(),
        }),
    );
    stack.script.push(
        "SignInService::exchange_mobile_code",
        Err::<SessionTokens, SignInError>(SignInError::RateLimited {
            retry_after: limited(),
        }),
    );
    let login = json!({"provider": "github", "return_to": "/f", "client_kind": "web"}).to_string();
    let grant = json!({"device_name": "d", "platform": "linux"}).to_string();
    let exchange = json!({"code": "c", "code_verifier": "v"}).to_string();

    // Act
    let answers = [
        stack
            .anonymous("POST", "/api/v1/auth/login-attempts", &login)
            .await,
        stack
            .anonymous("GET", "/api/v1/auth/providers/github/callback?code=c", "")
            .await,
        stack
            .anonymous("POST", "/api/v1/device-grants", &grant)
            .await,
        stack
            .anonymous("POST", "/api/v1/auth/mobile-code-exchanges", &exchange)
            .await,
    ];

    // Assert
    for answer in &answers {
        assert_limited(answer);
    }
    stack.stop().await;
}
