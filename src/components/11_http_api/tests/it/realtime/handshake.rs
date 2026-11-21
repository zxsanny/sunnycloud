use sunnycloud_http_api::RealtimeLimits;

use super::support::{Client, ORIGIN, Realtime, member_headers};
use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN};
use crate::harness::SESSION_COOKIE;

fn cookie() -> (&'static str, String) {
    ("Cookie", format!("{SESSION_COOKIE}={MEMBER_TOKEN}"))
}

#[tokio::test]
async fn a_cookie_session_from_the_public_origin_is_upgraded_and_hello_comes_first() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;

    // Act
    let mut client = realtime.connect().await;
    let first = client.frame().await;

    // Assert
    assert_eq!(first, super::support::HELLO);
    realtime.stop().await;
}

#[tokio::test]
async fn a_handshake_without_a_credential_is_401() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;

    // Act
    let refused = Client::open(realtime.addr(), &[("Origin", ORIGIN.to_owned())])
        .await
        .err();

    // Assert
    assert_eq!(refused.expect("refused").status, 401);
    realtime.stop().await;
}

#[tokio::test]
async fn a_cookie_handshake_whose_origin_is_not_the_public_one_is_403_and_never_upgraded() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let origins = [
        None,
        Some("https://evil.example"),
        Some("null"),
        Some("https://app.sunnycloud.test"),
        Some("http://sunnycloud.test"),
        Some("https://sunnycloud.test:8443"),
        Some("https://sunnycloud.test/"),
    ];

    // Act
    let mut answers = Vec::new();
    for origin in origins {
        let mut headers = vec![cookie()];
        headers.extend(origin.map(|origin| ("Origin", origin.to_owned())));
        let refused = Client::open(realtime.addr(), &headers).await.err();
        answers.push(refused.map(|refused| (refused.status, refused.code())));
    }

    // Assert
    let expected = Some((403, "csrf_rejected".to_owned()));
    assert_eq!(answers, vec![expected; origins.len()]);
    realtime.stop().await;
}

#[tokio::test]
async fn sec_fetch_site_alone_does_not_stand_in_for_the_origin() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let headers = [cookie(), ("Sec-Fetch-Site", "same-origin".to_owned())];

    // Act
    let refused = Client::open(realtime.addr(), &headers).await.err();

    // Assert
    assert_eq!(refused.expect("refused").status, 403);
    realtime.stop().await;
}

#[tokio::test]
async fn a_device_token_is_403() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let headers = [("Authorization", format!("Bearer {DEVICE_TOKEN}"))];

    // Act
    let refused = Client::open(realtime.addr(), &headers).await.err();

    // Assert
    assert_eq!(refused.expect("refused").status, 403);
    realtime.stop().await;
}

#[tokio::test]
async fn the_socket_after_the_cap_is_429_with_a_retry_after_and_a_closed_one_frees_its_slot() {
    // Arrange
    let limits = RealtimeLimits {
        max_connections_per_account: 2,
        ..RealtimeLimits::default()
    };
    let realtime = Realtime::start(limits).await;
    let first = realtime.connect().await;
    let _second = realtime.connect().await;

    // Act
    let refused = Client::open(realtime.addr(), &member_headers()).await.err();
    drop(first);
    let mut retry = None;
    for _ in 0..200 {
        retry = Client::open(realtime.addr(), &member_headers()).await.ok();
        if retry.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // Assert
    let refused = refused.expect("the third socket is refused");
    assert_eq!(
        (refused.status, refused.code()),
        (429, "rate_limited".to_owned())
    );
    assert!(refused.header("retry-after").is_some());
    assert!(
        retry.is_some(),
        "the slot of the closed socket is free again"
    );
    realtime.stop().await;
}

#[tokio::test]
async fn a_plain_get_that_is_no_upgrade_is_400() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let headers = [
        ("Cookie", format!("{SESSION_COOKIE}={MEMBER_TOKEN}")),
        ("Origin", ORIGIN.to_owned()),
    ];

    // Act
    let answer = crate::harness::router_server::request_with(
        realtime.addr(),
        "GET",
        "/api/v1/realtime",
        &[
            (headers[0].0, headers[0].1.as_str()),
            (headers[1].0, headers[1].1.as_str()),
        ],
    )
    .await;

    // Assert
    assert_eq!(answer.status, 400);
    realtime.stop().await;
}

#[test]
fn the_document_lists_the_realtime_operation_as_a_session_upgrade() {
    // Arrange
    let document: serde_json::Value =
        serde_json::from_slice(sunnycloud_http_api::current_document().as_bytes())
            .expect("document");

    // Act
    let operation = &document["paths"]["/api/v1/realtime"]["get"];

    // Assert
    assert_eq!(operation["x-auth-class"], "Session");
    assert!(operation["responses"]["101"].is_object(), "{operation}");
    assert!(operation["responses"]["200"].is_null());
}
