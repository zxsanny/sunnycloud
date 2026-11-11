use std::time::Duration;

use contracts::{ApprovalView, DeviceGrantError, DeviceInfo, DeviceToken};
use sc_ids::TokenKind;
use serde_json::json;

use super::support::*;
use crate::auth::support::{ADMIN_TOKEN, DEVICE_ACCOUNT, DEVICE_ID, MEMBER_ACCOUNT, MEMBER_TOKEN};

const REQUEST: &str = "DeviceGrantService::request";
const POLL: &str = "DeviceGrantService::poll";
const FORWARDED: [(&str, &str); 1] = [("x-forwarded-for", "198.51.100.7")];

fn behind_a_proxy(config: &mut sunnycloud_http_api::HttpConfig) {
    config.trusted_proxies = vec!["127.0.0.0/24".to_owned()];
}

fn poll_body() -> String {
    json!({"device_code": "dc-DEVICE_code"}).to_string()
}

#[tokio::test]
async fn a_grant_start_receives_the_forwarded_client_address_and_answers_the_codes() {
    // Arrange
    let stack = Stack::start_with(behind_a_proxy).await;
    let grant = device_grant();
    let user_code = user_code_text(&grant);
    stack.script.push(REQUEST, Ok::<_, DeviceGrantError>(grant));
    let body = json!({"device_name": "Studio PC", "platform": "windows"}).to_string();

    // Act
    let answer = stack
        .call("POST", "/api/v1/device-grants", &FORWARDED, &body)
        .await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.json(),
        json!({
            "device_code": "dc-DEVICE_code",
            "user_code": user_code,
            "interval": 5,
            "expires_in": 600,
        })
    );
    assert_eq!(
        stack.log(REQUEST),
        [format!("{REQUEST} Studio PC windows 198.51.100.7")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_four_poll_refusals_are_four_distinct_machine_readable_codes() {
    // Arrange
    let stack = Stack::start().await;
    let refusals = [
        DeviceGrantError::AuthorizationPending,
        DeviceGrantError::SlowDown,
        DeviceGrantError::AccessDenied,
        DeviceGrantError::ExpiredToken,
    ];
    for refusal in refusals {
        stack
            .script
            .push(POLL, Err::<DeviceToken, DeviceGrantError>(refusal));
    }

    // Act
    let mut answers = Vec::new();
    for _ in 0..4 {
        answers.push(
            stack
                .anonymous("POST", "/api/v1/device-tokens", &poll_body())
                .await,
        );
    }

    // Assert
    let codes: Vec<String> = answers.iter().map(|answer| answer.code()).collect();
    assert_eq!(
        codes,
        [
            "authorization_pending",
            "slow_down",
            "access_denied",
            "expired_token"
        ]
    );
    for answer in &answers {
        let body = answer.json();
        assert_eq!(answer.status, 400);
        assert_eq!(body["status"], 400);
        assert_eq!(body["error"], body["code"]);
        assert_eq!(
            body["type"],
            format!(
                "urn:sunnycloud:error:{}",
                body["code"].as_str().expect("code")
            )
        );
        assert_eq!(
            body["request_id"],
            answer.header("x-request-id").expect("id")
        );
        assert_eq!(
            answer.header("content-type"),
            Some("application/problem+json")
        );
    }
    assert_eq!(
        stack.log(POLL),
        ["DeviceGrantService::poll dc-DEVICE_code"; 4]
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_approved_poll_answers_the_device_token_once() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        POLL,
        Ok::<_, DeviceGrantError>(DeviceToken(token(TokenKind::DeviceToken, "dev-NEW_token"))),
    );

    // Act
    let answer = stack
        .anonymous("POST", "/api/v1/device-tokens", &poll_body())
        .await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.json(),
        json!({"access_token": "dev-NEW_token", "token_type": "Bearer"})
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_session_approves_or_denies_a_grant_by_the_id_in_the_path_and_a_device_cannot() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push("DeviceGrantService::decide", Ok::<(), DeviceGrantError>(()));
    stack
        .script
        .push("DeviceGrantService::decide", Ok::<(), DeviceGrantError>(()));
    let path = format!("/api/v1/device-grants/{GRANT}");

    // Act
    let approved = stack
        .as_member("PATCH", &path, r#"{"decision":"approve"}"#)
        .await;
    let denied = stack
        .as_member("PATCH", &path, r#"{"decision":"deny"}"#)
        .await;
    let by_device = stack
        .as_device("PATCH", &path, r#"{"decision":"approve"}"#)
        .await;
    let unknown = stack
        .as_member("PATCH", &path, r#"{"decision":"maybe"}"#)
        .await;

    // Assert
    assert_eq!((approved.status, denied.status), (204, 204));
    assert!(approved.body.is_empty());
    assert_eq!(by_device.status, 403);
    assert_eq!(unknown.status, 400);
    assert_eq!(
        stack.log("DeviceGrantService::decide"),
        [
            format!("DeviceGrantService::decide {MEMBER_ACCOUNT} {GRANT} Approve"),
            format!("DeviceGrantService::decide {MEMBER_ACCOUNT} {GRANT} Deny"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_decision_on_a_decided_or_unknown_grant_is_the_uniform_404() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "DeviceGrantService::decide",
        Err::<(), DeviceGrantError>(DeviceGrantError::AlreadyDecided),
    );

    // Act
    let answer = stack
        .as_member(
            "PATCH",
            &format!("/api/v1/device-grants/{GRANT}"),
            r#"{"decision":"approve"}"#,
        )
        .await;

    // Assert
    assert_eq!(answer.status, 404);
    assert_eq!(answer.code(), "not_found");
    stack.stop().await;
}

#[tokio::test]
async fn the_device_list_and_a_revocation_by_id_are_for_the_session_account() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "DeviceGrantService::list_devices",
        Ok::<_, DeviceGrantError>(vec![DeviceInfo {
            id: OTHER,
            name: "Studio PC".to_owned(),
            platform: "windows".to_owned(),
            created_at: WHEN,
            last_seen_at: None,
        }]),
    );
    stack.script.push(
        "DeviceGrantService::revoke_device",
        Ok::<(), DeviceGrantError>(()),
    );

    // Act
    let list = stack.as_member("GET", "/api/v1/devices", "").await;
    let revoke = stack
        .as_member("DELETE", &format!("/api/v1/devices/{OTHER}"), "")
        .await;
    let by_device = stack.as_device("GET", "/api/v1/devices", "").await;

    // Assert
    assert_eq!(list.status, 200);
    assert_eq!(
        list.json(),
        json!({"items": [{
            "id": OTHER, "name": "Studio PC", "platform": "windows",
            "created_at": WHEN_TEXT, "last_seen_at": null,
        }]})
    );
    assert_eq!(revoke.status, 204);
    assert_eq!(by_device.status, 403);
    assert_eq!(
        stack.log("DeviceGrantService"),
        [
            format!("DeviceGrantService::list_devices ({MEMBER_ACCOUNT})"),
            format!("DeviceGrantService::revoke_device ({MEMBER_ACCOUNT}, {OTHER})"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_revokes_itself_with_its_own_principal_and_a_session_is_refused_at_the_door()
{
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "DeviceGrantService::revoke_self",
        Ok::<(), DeviceGrantError>(()),
    );

    // Act
    let by_member = stack.as_member("DELETE", "/api/v1/devices/me", "").await;
    let by_admin = stack
        .with_token(ADMIN_TOKEN, "DELETE", "/api/v1/devices/me", "")
        .await;
    let by_device = stack.as_device("DELETE", "/api/v1/devices/me", "").await;

    // Assert
    assert_eq!((by_member.status, by_admin.status), (403, 403));
    assert_eq!(by_member.code(), "forbidden");
    assert_eq!(by_device.status, 204);
    assert_eq!(
        stack.log("DeviceGrantService"),
        [format!(
            "DeviceGrantService::revoke_self {DEVICE_ACCOUNT} Device {DEVICE_ID}"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_peer_that_is_not_a_trusted_proxy_cannot_forge_the_requester_address() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(REQUEST, Ok::<_, DeviceGrantError>(device_grant()));
    let body = json!({"device_name": "d", "platform": "linux"}).to_string();

    // Act: a forged header from a peer that is not a trusted proxy.
    let answer = stack
        .call("POST", "/api/v1/device-grants", &FORWARDED, &body)
        .await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(stack.log(REQUEST), [format!("{REQUEST} d linux 127.0.0.1")]);
    stack.stop().await;
}

const LOOKUP: &str = "DeviceGrantService::lookup_for_approval";

fn approval_view() -> ApprovalView {
    ApprovalView {
        grant_id: GRANT,
        expires_at: WHEN,
        device: "Studio PC".to_owned(),
        requester_ip: "203.0.113.9".parse().expect("address"),
        requester_location: Some("Kyiv, UA".to_owned()),
        approver_ip: "198.51.100.7".parse().expect("address"),
        approver_location: None,
        mismatch: true,
    }
}

#[tokio::test]
async fn the_approval_lookup_passes_the_principal_the_typed_code_and_the_forwarded_address() {
    // Arrange
    let stack = Stack::start_with(behind_a_proxy).await;
    stack
        .script
        .push(LOOKUP, Ok::<_, DeviceGrantError>(approval_view()));
    let auth = bearer(MEMBER_TOKEN);
    let headers = [
        ("authorization", auth.as_str()),
        ("x-forwarded-for", "198.51.100.7"),
    ];

    // Act
    let answer = stack
        .call(
            "GET",
            "/api/v1/device-grants?user_code=bcdf-ghjk",
            &headers,
            "",
        )
        .await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.json(),
        json!({
            "grant_id": GRANT,
            "expires_at": WHEN_TEXT,
            "device": "Studio PC",
            "requester_ip": "203.0.113.9",
            "requester_location": "Kyiv, UA",
            "approver_ip": "198.51.100.7",
            "approver_location": null,
            "mismatch": true,
        })
    );
    assert_eq!(
        stack.log(LOOKUP),
        [format!("{LOOKUP} {MEMBER_ACCOUNT} BCDFGHJK 198.51.100.7")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_approval_lookup_refuses_a_device_a_missing_code_and_a_code_that_cannot_exist() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let by_device = stack
        .as_device("GET", "/api/v1/device-grants?user_code=BCDFGHJK", "")
        .await;
    let missing = stack.as_member("GET", "/api/v1/device-grants", "").await;
    let malformed = stack
        .as_member("GET", "/api/v1/device-grants?user_code=BCDFGHJA", "")
        .await;

    // Assert
    assert_eq!(by_device.status, 403);
    assert_eq!(missing.status, 400);
    assert_eq!(malformed.status, 400);
    assert_eq!(stack.log(LOOKUP), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn the_approval_lookup_maps_an_unknown_code_to_404_and_the_abuse_limit_to_429() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        LOOKUP,
        Err::<ApprovalView, DeviceGrantError>(DeviceGrantError::UnknownCode),
    );
    stack.script.push(
        LOOKUP,
        Err::<ApprovalView, DeviceGrantError>(DeviceGrantError::RateLimited {
            retry_after: Duration::from_secs(30),
        }),
    );
    let path = "/api/v1/device-grants?user_code=BCDFGHJK";

    // Act
    let unknown = stack.as_member("GET", path, "").await;
    let limited = stack.as_member("GET", path, "").await;

    // Assert
    assert_eq!(unknown.status, 404);
    assert_eq!(unknown.code(), "not_found");
    assert_eq!(limited.status, 429);
    assert_eq!(limited.header("retry-after"), Some("30"));
    stack.stop().await;
}
