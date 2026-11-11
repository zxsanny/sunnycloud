use contracts::{CreatedShare, ShareError, ShareInfo};
use sc_clock::Timestamp;
use sc_ids::{RawToken, TokenKind};
use serde_json::json;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::MEMBER_ACCOUNT;

const SHARE: Uuid = Uuid::from_u128(0xF1);
const SECRET: &str = "sc_shr_PlaintextShownOnce";
const EXPIRES: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);

fn created() -> Result<CreatedShare, ShareError> {
    Ok(CreatedShare {
        id: SHARE,
        token: RawToken::from_presented(TokenKind::Share, SECRET.to_owned()),
        url_path: "/s/abc".to_owned(),
        expires_at: EXPIRES,
        has_password: true,
    })
}

fn listed() -> Result<Vec<ShareInfo>, ShareError> {
    Ok(vec![ShareInfo {
        id: SHARE,
        node_id: NODE,
        expires_at: EXPIRES,
        has_password: true,
        created_at: EXPIRES,
    }])
}

fn create_body() -> String {
    json!({"node_id": NODE, "expires_in": 3600, "password": "pw"}).to_string()
}

#[tokio::test]
async fn a_share_is_created_listed_and_revoked_for_the_account_of_the_session() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(CREATE, created());
    stack.script.push(LIST, listed());
    stack.script.push(REVOKE, Ok::<(), ShareError>(()));

    // Act
    let create = stack
        .as_member("POST", "/api/v1/shares", &create_body())
        .await;
    let list = stack
        .as_member("GET", &format!("/api/v1/shares?node_id={NODE}"), "")
        .await;
    let revoke = stack
        .as_member("DELETE", &format!("/api/v1/shares/{SHARE}"), "")
        .await;

    // Assert
    assert_eq!(create.status, 201);
    assert_eq!(create.json()["token"], SECRET);
    assert_eq!(create.json()["id"], SHARE.to_string());
    assert_eq!(create.json()["has_password"], true);
    assert_eq!(create.json()["expires_at"], "2026-11-06T09:11:12.123456Z");
    assert_eq!(list.status, 200);
    assert_eq!(list.json()["items"][0]["id"], SHARE.to_string());
    assert_eq!(revoke.status, 204);
    assert!(revoke.body.is_empty());
    assert_eq!(
        stack.log("ShareService"),
        [
            format!("{CREATE} ({MEMBER_ACCOUNT}, {NODE}, Some(3600s), Some(\"pw\"))"),
            format!("{LIST} ({MEMBER_ACCOUNT}, Some({NODE}))"),
            format!("{REVOKE} ({MEMBER_ACCOUNT}, {SHARE})"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_token_is_in_the_create_answer_only_and_never_in_a_listing() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(CREATE, created());
    stack.script.push(LIST, listed());

    // Act
    let create = stack
        .as_member("POST", "/api/v1/shares", &create_body())
        .await;
    let list = stack.as_member("GET", "/api/v1/shares", "").await;

    // Assert
    assert!(String::from_utf8_lossy(&create.body).contains(SECRET));
    let text = String::from_utf8_lossy(&list.body).into_owned();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("token"), "{text}");
    assert_eq!(
        stack.log(LIST),
        [format!("{LIST} ({MEMBER_ACCOUNT}, None)")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_cannot_manage_shares() {
    // Arrange
    let stack = Stack::start().await;
    let share = format!("/api/v1/shares/{SHARE}");

    // Act
    let create = stack
        .as_device("POST", "/api/v1/shares", &create_body())
        .await;
    let list = stack.as_device("GET", "/api/v1/shares", "").await;
    let revoke = stack.as_device("DELETE", &share, "").await;

    // Assert
    for answer in [&create, &list, &revoke] {
        assert_eq!(answer.status, 403);
        assert_eq!(answer.code(), "forbidden");
    }
    assert_eq!(stack.log("ShareService"), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn a_share_route_without_a_credential_is_401() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let create = stack
        .call("POST", "/api/v1/shares", &[], &create_body())
        .await;

    // Assert
    assert_eq!(create.status, 401);
    assert_eq!(create.code(), "unauthenticated");
    stack.stop().await;
}

#[tokio::test]
async fn a_cookie_session_must_prove_the_origin_to_create_or_revoke_a_share() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(CREATE, created());
    stack.script.push(REVOKE, Ok::<(), ShareError>(()));
    let cookie = cookie_header(crate::auth::support::MEMBER_TOKEN);
    let foreign = [
        ("cookie", cookie.as_str()),
        ("origin", "https://evil.example"),
    ];
    let same = [
        ("cookie", cookie.as_str()),
        ("origin", "https://sunnycloud.test"),
    ];
    let share = format!("/api/v1/shares/{SHARE}");

    // Act
    let refused_create = stack
        .call("POST", "/api/v1/shares", &foreign, &create_body())
        .await;
    let refused_revoke = stack.call("DELETE", &share, &foreign, "").await;
    let created_here = stack
        .call("POST", "/api/v1/shares", &same, &create_body())
        .await;
    let revoked_here = stack.call("DELETE", &share, &same, "").await;

    // Assert
    for answer in [&refused_create, &refused_revoke] {
        assert_eq!(answer.status, 403);
        assert_eq!(answer.code(), "csrf_rejected");
    }
    assert_eq!(created_here.status, 201);
    assert_eq!(revoked_here.status, 204);
    assert_eq!(stack.log("ShareService").len(), 2);
    stack.stop().await;
}

#[tokio::test]
async fn the_refusals_of_06_keep_their_codes_and_a_malformed_request_never_reaches_it() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        CREATE,
        Err::<CreatedShare, ShareError>(ShareError::TargetNotFound),
    );
    stack.script.push(
        CREATE,
        Err::<CreatedShare, ShareError>(ShareError::TargetNotShareable),
    );
    stack.script.push(
        CREATE,
        Err::<CreatedShare, ShareError>(ShareError::ExpiryOutOfRange),
    );
    stack
        .script
        .push(REVOKE, Err::<(), ShareError>(ShareError::NotFound));

    // Act
    let missing = stack
        .as_member("POST", "/api/v1/shares", &create_body())
        .await;
    let folder = stack
        .as_member("POST", "/api/v1/shares", &create_body())
        .await;
    let too_long = stack
        .as_member("POST", "/api/v1/shares", &create_body())
        .await;
    let foreign = stack
        .as_member("DELETE", &format!("/api/v1/shares/{SHARE}"), "")
        .await;
    let calls = stack.log("ShareService").len();
    let bad_body = stack
        .as_member("POST", "/api/v1/shares", r#"{"node_id":"7","extra":1}"#)
        .await;
    let bad_id = stack
        .as_member("DELETE", "/api/v1/shares/not-a-uuid", "")
        .await;
    let bad_filter = stack.as_member("GET", "/api/v1/shares?node_id=7", "").await;

    // Assert
    assert_eq!(
        (missing.status, missing.code().as_str()),
        (404, "not_found")
    );
    assert_eq!(
        (folder.status, folder.code().as_str()),
        (422, "target_not_shareable")
    );
    assert_eq!(too_long.status, 400);
    assert_eq!(too_long.code(), "invalid_request");
    assert_eq!(
        (foreign.status, foreign.code().as_str()),
        (404, "not_found")
    );
    assert_eq!(calls, 4);
    for answer in [&bad_body, &bad_id, &bad_filter] {
        assert_eq!(answer.status, 400);
        assert_eq!(answer.code(), "invalid_request");
    }
    assert_eq!(stack.log("ShareService").len(), calls);
    stack.stop().await;
}
