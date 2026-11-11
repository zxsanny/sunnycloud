use contracts::{SessionError, SessionInfo};
use serde_json::json;

use super::support::*;
use crate::auth::support::{MEMBER_ACCOUNT, MEMBER_SESSION};

const REVOKE: &str = "SessionService::revoke";

#[tokio::test]
async fn the_session_list_is_the_callers_own() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SessionService::list",
        Ok::<_, SessionError>(vec![SessionInfo {
            id: OTHER,
            label: "Firefox".to_owned(),
            created_at: WHEN,
            last_seen_at: WHEN,
        }]),
    );

    // Act
    let answer = stack.as_member("GET", "/api/v1/sessions", "").await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.json(),
        json!({"items": [{
            "id": OTHER, "label": "Firefox", "created_at": WHEN_TEXT, "last_seen_at": WHEN_TEXT,
        }]})
    );
    assert_eq!(
        stack.log("SessionService::list"),
        [format!("SessionService::list ({MEMBER_ACCOUNT})")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn revoking_the_current_session_passes_the_callers_own_session_id() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(REVOKE, Ok::<(), SessionError>(()));

    // Act
    let answer = stack
        .as_member("DELETE", "/api/v1/sessions/current", "")
        .await;

    // Assert
    assert_eq!(answer.status, 204);
    assert!(answer.body.is_empty());
    assert_eq!(
        stack.log(REVOKE),
        [format!(
            "{REVOKE} ({MEMBER_ACCOUNT}, {MEMBER_SESSION}, User)"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn revoking_a_session_by_id_passes_the_account_of_the_caller_and_the_id_of_the_path() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(REVOKE, Ok::<(), SessionError>(()));

    // Act
    let answer = stack
        .as_member("DELETE", &format!("/api/v1/sessions/{OTHER}"), "")
        .await;
    let malformed = stack
        .as_member("DELETE", "/api/v1/sessions/not-a-uuid", "")
        .await;

    // Assert
    assert_eq!(answer.status, 204);
    assert_eq!(
        (malformed.status, malformed.code().as_str()),
        (400, "invalid_request")
    );
    assert_eq!(
        stack.log(REVOKE),
        [format!("{REVOKE} ({MEMBER_ACCOUNT}, {OTHER}, User)")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_cannot_list_or_revoke_sessions() {
    // Arrange
    let stack = Stack::start().await;
    let by_id = format!("/api/v1/sessions/{OTHER}");

    // Act
    let answers = [
        stack.as_device("GET", "/api/v1/sessions", "").await,
        stack.as_device("DELETE", &by_id, "").await,
        stack
            .as_device("DELETE", "/api/v1/sessions/current", "")
            .await,
    ];

    // Assert
    for answer in &answers {
        assert_eq!((answer.status, answer.code().as_str()), (403, "forbidden"));
    }
    assert_eq!(stack.calls(), 0);
    stack.stop().await;
}
