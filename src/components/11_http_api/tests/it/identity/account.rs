use contracts::{
    AccountServiceError, AccountView, DeletionAccepted, DeletionError, IdentityView, Role,
};
use serde_json::json;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};

const ROOT: Uuid = Uuid::from_u128(0xD0);
const VIEW: &str = "AccountService::view";
const DELETE: &str = "AccountDeletionService::request";

fn account() -> AccountView {
    AccountView {
        id: MEMBER_ACCOUNT,
        display_name: "Ada".to_owned(),
        role: Role::User,
        identities: vec![IdentityView {
            provider: "github".to_owned(),
            email: Some("ada@example.test".to_owned()),
        }],
        quota: quota(),
        root_node_id: ROOT,
    }
}

#[tokio::test]
async fn the_own_account_view_and_quota_are_answered_for_a_session_and_for_a_device() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(VIEW, Ok::<_, AccountServiceError>(account()));
    stack.script.push(
        "AccountService::quota_view",
        Ok::<_, AccountServiceError>(quota()),
    );
    stack.script.push(
        "AccountService::quota_view",
        Ok::<_, AccountServiceError>(quota()),
    );

    // Act
    let view = stack.as_member("GET", "/api/v1/accounts/me", "").await;
    let quota_by_session = stack
        .as_member("GET", "/api/v1/accounts/me/quota", "")
        .await;
    let quota_by_device = stack
        .as_device("GET", "/api/v1/accounts/me/quota", "")
        .await;

    // Assert
    assert_eq!(view.status, 200);
    assert_eq!(
        view.json(),
        json!({
            "id": MEMBER_ACCOUNT, "display_name": "Ada", "role": "user",
            "identities": [{"provider": "github", "email": "ada@example.test"}],
            "quota": {"limit_bytes": null, "effective_limit": 1_000_000, "used": 400, "reserved": 25},
            "root_node_id": ROOT,
        })
    );
    let expected =
        json!({"limit_bytes": null, "effective_limit": 1_000_000, "used": 400, "reserved": 25});
    assert_eq!(quota_by_session.json(), expected);
    assert_eq!(quota_by_device.json(), expected);
    assert_eq!(
        stack.log("AccountService::quota_view").len(),
        2,
        "one port call per request"
    );
    let by_device = stack.log("AccountService::quota_view")[1].clone();
    assert!(
        by_device.contains(&DEVICE_ACCOUNT.to_string()),
        "{by_device}"
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_account_deletion_is_accepted_with_its_deadline_and_the_confirmation_reaches_07() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        DELETE,
        Ok::<_, DeletionError>(DeletionAccepted { deadline: WHEN }),
    );

    // Act
    let answer = stack
        .as_member("DELETE", "/api/v1/accounts/me", r#"{"confirmation":"ada"}"#)
        .await;

    // Assert
    assert_eq!(answer.status, 202);
    assert_eq!(answer.json(), json!({"deadline": WHEN_TEXT}));
    let log = stack.log(DELETE);
    assert_eq!(log.len(), 1);
    assert!(
        log[0].contains(&MEMBER_ACCOUNT.to_string()) && log[0].ends_with("\"ada\")"),
        "{log:?}"
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_last_admin_and_a_wrong_confirmation_are_answered_as_the_port_says() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        DELETE,
        Err::<DeletionAccepted, DeletionError>(DeletionError::LastAdmin),
    );
    stack.script.push(
        DELETE,
        Err::<DeletionAccepted, DeletionError>(DeletionError::ConfirmationMismatch),
    );

    // Act
    let last = stack
        .as_member("DELETE", "/api/v1/accounts/me", r#"{"confirmation":"ada"}"#)
        .await;
    let mismatch = stack
        .as_member("DELETE", "/api/v1/accounts/me", r#"{"confirmation":"bob"}"#)
        .await;

    // Assert
    assert_eq!((last.status, last.code().as_str()), (409, "last_admin"));
    assert_eq!(mismatch.status, 400);
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_cannot_delete_the_account_and_a_body_that_does_not_bind_reaches_no_port() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let by_device = stack
        .as_device("DELETE", "/api/v1/accounts/me", r#"{"confirmation":"ada"}"#)
        .await;
    let no_member = stack.as_member("DELETE", "/api/v1/accounts/me", "{}").await;

    // Assert
    assert_eq!(
        (by_device.status, by_device.code().as_str()),
        (403, "forbidden")
    );
    assert_eq!(
        (no_member.status, no_member.code().as_str()),
        (400, "invalid_request")
    );
    assert_eq!(stack.calls(), 0);
    stack.stop().await;
}
