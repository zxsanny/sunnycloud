//! AC-4 to AC-6 of AZ-3959: the version header and 426, TLS 1.3 and HTTP/2 only, no cross-host redirect,
//! and certificates checked against the trust store rather than skipped.

mod connect_support;
mod request_support;
mod support;

use connect_support::{client, config, connect, failure, valid_info};
use request_support::{ACCESS, SentinelCredentials, get};
use serde_json::{Value, json};
use support::{Answer, Fixture, Route, Script, TlsMode};
use sync_core::{ConnectError, HttpSyncClient, Mapped, SyncApi, SyncError};

const CLIENT_HEADER: &str = "x-sunnycloud-client";

fn script() -> Script {
    Script::new().always(Route::ServerInfo, Answer::json(200, &valid_info()))
}

fn ok(body: &Value) -> Answer {
    Answer::json(200, body)
}

#[tokio::test]
async fn every_request_but_server_info_carries_the_client_header() {
    let fixture = Fixture::start(script().always(Route::RootNode, ok(&json!({})))).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);

    for _ in 0..3 {
        client
            .request_path()
            .send("root", get("/api/v1/nodes/root", 1))
            .await
            .expect("answer");
    }

    let log = fixture.requests();
    assert_eq!(log.len(), 4);
    assert_eq!(log[0].route, Some(Route::ServerInfo));
    assert_eq!(log[0].header(CLIENT_HEADER), None);
    assert_eq!(log[0].header("authorization"), None);
    for request in &log[1..] {
        assert_eq!(request.header(CLIENT_HEADER), Some("desktop/1.2.3"));
        let authorization = format!("Bearer {ACCESS}");
        assert_eq!(
            request.header("authorization"),
            Some(authorization.as_str())
        );
        assert!(
            request
                .header("accept")
                .is_some_and(|accept| accept.contains("application/json"))
        );
    }
}

#[tokio::test]
async fn a_callers_headers_cannot_replace_the_client_header_or_the_credential() {
    let fixture = Fixture::start(script().always(Route::RootNode, ok(&json!({})))).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);
    let mut request = get("/api/v1/nodes/root", 1);
    request.headers = vec![
        (CLIENT_HEADER.to_owned(), "web/9.9.9".to_owned()),
        ("Authorization".to_owned(), "Bearer forged".to_owned()),
        ("Accept".to_owned(), "application/octet-stream".to_owned()),
    ];

    client
        .request_path()
        .send("root", request)
        .await
        .expect("answer");

    let sent = &fixture.requests()[1];
    assert_eq!(sent.header(CLIENT_HEADER), Some("desktop/1.2.3"));
    assert_eq!(sent.header("authorization"), Some("Bearer ACC-SENTINEL"));
    assert_eq!(sent.header("accept"), Some("application/octet-stream"));
}

#[tokio::test]
async fn a_426_is_upgrade_required_with_the_server_text_and_is_never_retried() {
    let upgrade = Answer::problem(sc_error::ErrorCode::UpgradeRequired)
        .extra("detail", json!("Please upgrade to 2.0.0"))
        .extra("min_version", json!("2.0.0"))
        .extra("update_url", json!("https://sunnycloud.test/download"));
    let script = script()
        .on(Route::RootNode, ok(&json!({})))
        .on(Route::RootNode, upgrade)
        .on(Route::RootNode, Answer::status(426))
        .always(Route::RootNode, ok(&json!({})));
    let fixture = Fixture::start(script).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);
    let send = || {
        client
            .request_path()
            .send("root", get("/api/v1/nodes/root", 5))
    };

    let first = send().await;
    let second = send().await;
    let third = send().await;

    assert!(first.is_ok());
    match second {
        Err(Mapped::Error(SyncError::UpgradeRequired {
            message,
            min_version,
            update_url,
        })) => {
            assert_eq!(message, "Please upgrade to 2.0.0");
            assert_eq!(min_version.as_deref(), Some("2.0.0"));
            assert_eq!(
                update_url.as_deref(),
                Some("https://sunnycloud.test/download")
            );
        }
        other => panic!("expected UpgradeRequired, got {other:?}"),
    }
    assert!(matches!(
        third,
        Err(Mapped::Error(SyncError::UpgradeRequired { .. }))
    ));
    let root_requests = fixture
        .requests()
        .iter()
        .filter(|request| request.route == Some(Route::RootNode))
        .count();
    assert_eq!(root_requests, 3, "a 426 is sent once, not retried");
}

#[tokio::test]
async fn a_tls12_only_server_is_refused_and_no_request_reaches_it() {
    let fixture = Fixture::builder(script())
        .tls(TlsMode::Tls12Only)
        .start()
        .await;

    let result = connect(&fixture, config(&fixture.url())).await;

    assert_eq!(failure(result), ConnectError::Unreachable);
    assert!(fixture.requests().is_empty());
    assert!(
        fixture.connections().is_empty(),
        "no TLS session was completed"
    );
}

#[tokio::test]
async fn a_server_without_h2_is_refused_and_no_request_reaches_it() {
    let fixture = Fixture::builder(script())
        .tls(TlsMode::Tls13NoH2)
        .start()
        .await;

    let result = connect(&fixture, config(&fixture.url())).await;

    assert_eq!(failure(result), ConnectError::Unreachable);
    assert!(fixture.requests().is_empty());
}

#[tokio::test]
async fn the_session_is_tls13_over_h2() {
    let fixture = Fixture::start(script()).await;

    client(connect(&fixture, config(&fixture.url())).await);

    let sessions = fixture.connections();
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].tls_version,
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    assert_eq!(sessions[0].alpn.as_deref(), Some("h2"));
}

#[tokio::test]
async fn all_requests_share_one_connection() {
    let fixture = Fixture::start(script().always(Route::RootNode, ok(&json!({})))).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);

    for _ in 0..4 {
        client
            .request_path()
            .send("root", get("/api/v1/nodes/root", 1))
            .await
            .expect("answer");
    }

    assert_eq!(fixture.connections().len(), 1);
}

#[tokio::test]
async fn a_certificate_outside_the_trust_store_is_refused() {
    let fixture = Fixture::start(script()).await;

    let result = HttpSyncClient::connect(config(&fixture.url()), SentinelCredentials::new()).await;

    assert_eq!(failure(result), ConnectError::Unreachable);
    assert!(fixture.requests().is_empty());
}

#[tokio::test]
async fn a_certificate_signed_by_another_ca_is_refused() {
    let fixture = Fixture::start(script()).await;
    let other = Fixture::start(script()).await;

    let result = connect(&other, config(&fixture.url())).await;

    assert_eq!(failure(result), ConnectError::Unreachable);
    assert!(fixture.requests().is_empty());
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_the_other_host_sees_nothing() {
    let script = script().on(
        Route::RootNode,
        Answer::redirect("{catcher}/api/v1/nodes/root"),
    );
    let fixture = Fixture::start(script).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);

    let answer = client
        .request_path()
        .send("root", get("/api/v1/nodes/root", 1))
        .await;

    assert!(
        matches!(answer, Err(Mapped::Error(SyncError::Unexpected { .. }))),
        "the 302 is an answer like any other non-2xx: {answer:?}"
    );
    assert!(fixture.catcher_requests().is_empty());
    assert!(!fixture.catcher_saw_authorization());
}

#[tokio::test]
async fn a_target_that_leaves_the_api_or_the_origin_is_not_sent() {
    let fixture = Fixture::start(script()).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);

    for target in ["/other", "//evil.test/api/v1/x", "api/v1/x", "/api/v2/x"] {
        let answer = client.request_path().send("bad", get(target, 1)).await;
        assert!(
            matches!(answer, Err(Mapped::Error(SyncError::Transport(_)))),
            "{target}: {answer:?}"
        );
    }

    assert_eq!(fixture.requests().len(), 1, "only server-info was sent");
}

#[tokio::test]
async fn cancel_ends_the_next_request_with_cancelled() {
    let fixture = Fixture::start(script().always(Route::RootNode, ok(&json!({})))).await;
    let (client, _) = client(connect(&fixture, config(&fixture.url())).await);

    client.cancel();
    let answer = client
        .request_path()
        .send("root", get("/api/v1/nodes/root", 1))
        .await;

    assert!(matches!(answer, Err(Mapped::Error(SyncError::Cancelled))));
    assert_eq!(fixture.requests().len(), 1);
}
