//! AZ-3926 AC-2, AC-3 and AC-6 at the HTTP boundary (Mode S): the base64 JSON wire, the quoted `ETag`, the
//! `If-Match` rule (428 with no port call, the tag passed unquoted, 412 from the port), the auth classes and CSRF,
//! and each port error answered once with its `sc_error` row.

use contracts::{CreatedVault, ETag, VaultError, VaultHeader};
use serde_json::json;

use super::support::{H0_TAG, H1_TAG, ROOT, Stack, VAULT, quoted};
use crate::auth::support::MEMBER_ACCOUNT;

const VAULTS: &str = "/api/v1/vaults";
const CREATE: &str = "VaultService::create_vault";
const GET: &str = "VaultService::get_header";
const REPLACE: &str = "VaultService::replace_header";
const REPORT: &str = "VaultService::report_unlock_failure";

fn vault_path() -> String {
    format!("{VAULTS}/{VAULT}")
}

fn header_path() -> String {
    format!("{VAULTS}/{VAULT}/header")
}

fn report_path() -> String {
    format!("{VAULTS}/{VAULT}/unlock-failures")
}

/// `{"header": base64("H1")}`.
const H1_BODY: &str = r#"{"header":"SDE="}"#;

#[tokio::test]
async fn create_answers_201_with_the_quoted_etag_and_passes_the_decoded_header() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        CREATE,
        Ok::<_, VaultError>(CreatedVault {
            vault_id: VAULT,
            etag: ETag(H0_TAG.to_owned()),
        }),
    );
    let body = json!({"root_node_id": ROOT, "header": "SDA="}).to_string();

    // Act
    let answer = stack.as_member("POST", VAULTS, &[], &body).await;

    // Assert
    assert_eq!(answer.status, 201);
    assert_eq!(answer.header("etag"), Some(quoted(H0_TAG).as_str()));
    assert_eq!(answer.json(), json!({"id": VAULT}));
    assert_eq!(
        stack.vault_calls(),
        [format!("{CREATE} ({MEMBER_ACCOUNT}, [72, 48], {ROOT})")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn get_answers_the_header_in_base64_with_its_root_and_quoted_etag() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        GET,
        Ok::<_, VaultError>(VaultHeader {
            header: b"H0".to_vec(),
            etag: ETag(H0_TAG.to_owned()),
            root_node_id: ROOT,
        }),
    );

    // Act
    let answer = stack.as_member("GET", &vault_path(), &[], "").await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("etag"), Some(quoted(H0_TAG).as_str()));
    assert_eq!(
        answer.json(),
        json!({"id": VAULT, "root_node_id": ROOT, "header": "SDA="})
    );
    stack.stop().await;
}

#[tokio::test]
async fn replace_without_if_match_is_428_and_reaches_no_port() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let answer = stack.as_member("PUT", &header_path(), &[], H1_BODY).await;

    // Assert
    assert_eq!(answer.status, 428);
    assert_eq!(answer.code(), "precondition_required");
    assert_eq!(stack.vault_calls(), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn replace_passes_the_tag_unquoted_and_maps_the_ports_412_and_200() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(REPLACE, Err::<ETag, _>(VaultError::PreconditionFailed));
    stack
        .script
        .push(REPLACE, Ok::<_, VaultError>(ETag(H1_TAG.to_owned())));
    let current = quoted(H0_TAG);

    // Act
    let stale = stack
        .as_member("PUT", &header_path(), &[("if-match", "\"0000\"")], H1_BODY)
        .await;
    let fresh = stack
        .as_member(
            "PUT",
            &header_path(),
            &[("if-match", current.as_str())],
            H1_BODY,
        )
        .await;

    // Assert
    assert_eq!(stale.status, 412);
    assert_eq!(stale.code(), "precondition_failed");
    assert_eq!(fresh.status, 200);
    assert_eq!(fresh.header("etag"), Some(quoted(H1_TAG).as_str()));
    assert!(fresh.body.is_empty());
    assert_eq!(
        stack.vault_calls(),
        [
            format!("{REPLACE} ({MEMBER_ACCOUNT}, {VAULT}, [72, 49], ETag(\"0000\"))"),
            format!("{REPLACE} ({MEMBER_ACCOUNT}, {VAULT}, [72, 49], ETag(\"{H0_TAG}\"))"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_unquoted_or_empty_if_match_reaches_the_port_as_a_tag_that_matches_nothing() {
    // Arrange
    let stack = Stack::start().await;
    for _ in 0..2 {
        stack
            .script
            .push(REPLACE, Err::<ETag, _>(VaultError::PreconditionFailed));
    }

    // Act
    let unquoted = stack
        .as_member("PUT", &header_path(), &[("if-match", H0_TAG)], H1_BODY)
        .await;
    let empty = stack
        .as_member("PUT", &header_path(), &[("if-match", "")], H1_BODY)
        .await;

    // Assert
    assert_eq!((unquoted.status, empty.status), (412, 412));
    let empty_tag = format!("{REPLACE} ({MEMBER_ACCOUNT}, {VAULT}, [72, 49], ETag(\"\"))");
    assert_eq!(stack.vault_calls(), [empty_tag.clone(), empty_tag]);
    stack.stop().await;
}

#[tokio::test]
async fn a_report_answers_204_and_its_body_reaches_no_port_and_no_log() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(REPORT, Ok::<(), VaultError>(()));
    let secret = "correct horse battery staple 0123456789abcdef0123456789abcdef";
    let body = json!({"passphrase": secret}).to_string();

    // Act
    let answer = stack
        .as_member(
            "POST",
            &report_path(),
            &[("content-type", "application/json")],
            &body,
        )
        .await;

    // Assert
    assert_eq!(answer.status, 204);
    assert!(answer.body.is_empty());
    assert_eq!(
        stack.script.log(),
        [format!("{REPORT} ({MEMBER_ACCOUNT}, {VAULT})")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_gets_403_on_create_and_replace_and_no_credential_gets_401() {
    // Arrange
    let stack = Stack::start().await;
    let create_body = json!({"root_node_id": ROOT, "header": "SDA="}).to_string();
    let tag = quoted(H0_TAG);

    // Act
    let device_create = stack.as_device("POST", VAULTS, &create_body).await;
    let device_replace = stack.as_device("PUT", &header_path(), H1_BODY).await;
    let anonymous = [
        stack.call("POST", VAULTS, &[], &create_body).await,
        stack.call("GET", &vault_path(), &[], "").await,
        stack
            .call(
                "PUT",
                &header_path(),
                &[("if-match", tag.as_str())],
                H1_BODY,
            )
            .await,
        stack.call("POST", &report_path(), &[], "").await,
    ];

    // Assert
    assert_eq!((device_create.status, device_replace.status), (403, 403));
    assert_eq!(
        anonymous
            .iter()
            .map(|reply| reply.status)
            .collect::<Vec<_>>(),
        [401, 401, 401, 401]
    );
    assert_eq!(stack.vault_calls(), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn a_cross_origin_cookie_put_is_403_csrf_rejected_and_a_same_origin_one_passes() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(REPLACE, Ok::<_, VaultError>(ETag(H1_TAG.to_owned())));
    let tag = quoted(H0_TAG);

    // Act
    let cross = stack
        .with_cookie(
            "PUT",
            &header_path(),
            &[
                ("if-match", tag.as_str()),
                ("origin", "https://evil.example"),
            ],
            H1_BODY,
        )
        .await;
    let calls_after_cross = stack.vault_calls().len();
    let same = stack
        .with_cookie(
            "PUT",
            &header_path(),
            &[
                ("if-match", tag.as_str()),
                ("sec-fetch-site", "same-origin"),
            ],
            H1_BODY,
        )
        .await;

    // Assert
    assert_eq!(cross.status, 403);
    assert_eq!(cross.code(), "csrf_rejected");
    assert_eq!(calls_after_cross, 0);
    assert_eq!(same.status, 200);
    stack.stop().await;
}

#[tokio::test]
async fn each_port_error_is_answered_once_with_its_code_and_no_detail() {
    // Arrange
    let stack = Stack::start().await;
    let errors = [
        (VaultError::NotFound, 404, "not_found"),
        (VaultError::HeaderTooLarge, 413, "vault_header_too_large"),
        (VaultError::VaultExists, 409, "vault_exists"),
        (VaultError::PreconditionFailed, 412, "precondition_failed"),
        (
            VaultError::Unexpected("vault key K3Y at /var/lib/x".into()),
            500,
            "internal",
        ),
    ];
    let expected: Vec<(u16, &str)> = errors
        .iter()
        .map(|(_, status, code)| (*status, *code))
        .collect();
    for (error, _, _) in errors {
        stack.script.push(CREATE, Err::<CreatedVault, _>(error));
    }
    let body = json!({"root_node_id": ROOT, "header": "SDA="}).to_string();

    // Act
    let mut found = Vec::new();
    let mut texts = Vec::new();
    for _ in 0..expected.len() {
        let answer = stack.as_member("POST", VAULTS, &[], &body).await;
        texts.push(String::from_utf8_lossy(&answer.body).into_owned());
        found.push((answer.status, answer.code()));
    }

    // Assert
    let found: Vec<(u16, &str)> = found.iter().map(|(s, c)| (*s, c.as_str())).collect();
    assert_eq!(found, expected);
    for text in texts {
        for leak in [
            "SDA=",
            "K3Y",
            "/var/lib",
            &ROOT.to_string(),
            &MEMBER_ACCOUNT.to_string(),
        ] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }
    stack.stop().await;
}

#[tokio::test]
async fn a_header_that_is_not_base64_is_refused_before_the_port() {
    // Arrange
    let stack = Stack::start().await;
    let body = json!({"root_node_id": ROOT, "header": "not base64!"}).to_string();

    // Act
    let answer = stack.as_member("POST", VAULTS, &[], &body).await;

    // Assert
    assert_eq!(answer.status, 400);
    assert_eq!(answer.code(), "invalid_request");
    assert_eq!(
        answer.json()["problems"],
        json!([{"field": "header", "problem": "not_base64"}])
    );
    assert_eq!(stack.vault_calls(), Vec::<String>::new());
    stack.stop().await;
}
