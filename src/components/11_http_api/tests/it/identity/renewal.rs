//! The browser's session renewal: the refresh cookie goes to one route, from the app's own origin, and comes
//! back rotated together with a new session cookie.

use contracts::{SessionError, SessionTokens};

use super::support::*;
use crate::worker::support::Reply;

const RENEW: &str = "/api/v1/auth/session-renewals";
const REFRESH_CALL: &str = "SessionService::refresh";
const SAME_ORIGIN: (&str, &str) = ("sec-fetch-site", "same-origin");
const COOKIE_NAME: &str = "__Secure-refresh";

fn cookie_lines(reply: &Reply) -> Vec<&str> {
    reply
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect()
}

#[tokio::test]
async fn a_renewal_rotates_the_refresh_cookie_and_sets_a_new_session_cookie_with_no_body() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(REFRESH_CALL, Ok::<_, SessionError>(session_tokens()));
    let cookie = format!("{COOKIE_NAME}=ref-OLD");

    // Act
    let answer = stack
        .call(
            "POST",
            RENEW,
            &[("cookie", cookie.as_str()), SAME_ORIGIN],
            "",
        )
        .await;

    // Assert
    assert_eq!(answer.status, 204);
    assert_eq!(
        cookie_lines(&answer),
        [
            "__Host-test-session=acc-ESS_token; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=900",
            "__Secure-refresh=ref-RESH_token; Secure; HttpOnly; SameSite=Strict; \
             Path=/api/v1/auth/session-renewals; Max-Age=2592000",
        ]
    );
    assert_eq!(stack.log(REFRESH_CALL), [format!("{REFRESH_CALL} ref-OLD")]);
    stack.stop().await;
}

#[tokio::test]
async fn a_refresh_token_01_refuses_is_401_and_the_cookie_is_cleared() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        REFRESH_CALL,
        Err::<SessionTokens, SessionError>(SessionError::ReuseDetected),
    );
    let cookie = format!("{COOKIE_NAME}=ref-OLD");

    // Act
    let answer = stack
        .call(
            "POST",
            RENEW,
            &[("cookie", cookie.as_str()), SAME_ORIGIN],
            "",
        )
        .await;

    // Assert
    assert_eq!(answer.status, 401);
    assert_eq!(
        cookie_lines(&answer),
        ["__Secure-refresh=; Secure; HttpOnly; SameSite=Strict; \
             Path=/api/v1/auth/session-renewals; Max-Age=0"]
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_unexpected_failure_keeps_the_cookie_so_an_outage_does_not_sign_the_browser_out() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        REFRESH_CALL,
        Err::<SessionTokens, SessionError>(SessionError::Unexpected("database down".into())),
    );
    let cookie = format!("{COOKIE_NAME}=ref-OLD");

    // Act
    let answer = stack
        .call(
            "POST",
            RENEW,
            &[("cookie", cookie.as_str()), SAME_ORIGIN],
            "",
        )
        .await;

    // Assert
    assert_eq!((answer.status, cookie_lines(&answer).len()), (500, 0));
    stack.stop().await;
}

#[tokio::test]
async fn a_request_without_the_cookie_is_401_and_reaches_no_port() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let answer = stack.call("POST", RENEW, &[SAME_ORIGIN], "").await;

    // Assert
    assert_eq!((answer.status, stack.calls()), (401, 0));
    stack.stop().await;
}

#[tokio::test]
async fn a_request_that_does_not_prove_the_apps_origin_is_refused_before_the_port() {
    // Arrange
    let stack = Stack::start().await;
    let cookie = format!("{COOKIE_NAME}=ref-OLD");
    let foreign = ("origin", "https://evil.example");

    // Act
    let answer = stack
        .call("POST", RENEW, &[("cookie", cookie.as_str()), foreign], "")
        .await;

    // Assert
    assert_eq!(
        (answer.status, answer.code(), stack.calls()),
        (403, "csrf_rejected".to_owned(), 0)
    );
    stack.stop().await;
}
