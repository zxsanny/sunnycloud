use super::support::{
    Gated, PROTECTED, PUBLIC, SERVER_INFO, UPDATE_URL, gated_config, get, get_as,
};
use crate::auth::support::{MEMBER_TOKEN, PROBES, path_of};
use crate::harness::metric;
use sunnycloud_http_api::registry::AuthClass;

const SUPPORTED_ANDROID: &str =
    "sunnycloud_sync_client_versions_total{client=\"android\",supported=\"yes\"}";
const UNSUPPORTED_ANDROID: &str =
    "sunnycloud_sync_client_versions_total{client=\"android\",supported=\"no\"}";

#[tokio::test]
async fn an_old_client_gets_426_with_the_minimum_and_the_update_url_and_no_port_is_called() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;

    // Act
    let old = get_as(
        server.public.addr,
        PROTECTED,
        Some("android/1.1.9"),
        MEMBER_TOKEN,
    )
    .await;

    // Assert
    assert_eq!(old.status, 426);
    assert_eq!(old.header("content-type"), Some("application/problem+json"));
    let body = old.json();
    assert_eq!(body["code"], "upgrade_required");
    assert_eq!(body["status"], 426);
    assert_eq!(body["min_version"], "1.2.0");
    assert_eq!(body["update_url"], UPDATE_URL);
    let text = format!("{} {}", body["title"], body["detail"]).to_lowercase();
    assert!(text.contains("upgrade"), "{text}");
    assert_eq!(body["request_id"].as_str(), old.header("x-request-id"));
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn equal_and_higher_versions_pass_the_gate_to_the_handler() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;

    // Act
    let equal = get_as(
        server.public.addr,
        PROTECTED,
        Some("web/1.2.0"),
        MEMBER_TOKEN,
    )
    .await;
    let higher = get_as(
        server.public.addr,
        PROTECTED,
        Some("desktop/1.3.0"),
        MEMBER_TOKEN,
    )
    .await;

    // Assert
    assert_eq!((equal.status, higher.status), (200, 200));
    assert_eq!(server.authenticator.calls().len(), 2);
    server.stop().await;
}

#[tokio::test]
async fn each_family_is_held_to_its_own_minimum() {
    // Arrange
    let server = Gated::start(gated_config("web=1.0.0,android=1.5.0")).await;

    // Act
    let web = get_as(
        server.public.addr,
        PROTECTED,
        Some("web/1.0.0"),
        MEMBER_TOKEN,
    )
    .await;
    let android = get_as(
        server.public.addr,
        PROTECTED,
        Some("android/1.4.9"),
        MEMBER_TOKEN,
    )
    .await;

    // Assert
    assert_eq!(web.status, 200);
    assert_eq!(android.status, 426);
    assert_eq!(android.json()["min_version"], "1.5.0");
    server.stop().await;
}

#[tokio::test]
async fn a_family_the_list_does_not_name_has_no_minimum() {
    // Arrange
    let server = Gated::start(gated_config("web=1.0.0")).await;

    // Act
    let ios = get_as(
        server.public.addr,
        PROTECTED,
        Some("ios/0.0.1"),
        MEMBER_TOKEN,
    )
    .await;

    // Assert
    assert_eq!(ios.status, 200);
    server.stop().await;
}

#[tokio::test]
async fn public_routes_and_server_info_pass_any_version_and_the_gate_runs_before_auth() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let addr = server.public.addr;
    let old = [("x-sunnycloud-client", "ios/0.9.0")];

    // Act
    let public = get(addr, PUBLIC, &old).await;
    let info = get(addr, SERVER_INFO, &old).await;
    let unauthenticated = get(addr, PROTECTED, &[("x-sunnycloud-client", "android/1.1.9")]).await;

    // Assert
    assert_eq!((public.status, info.status), (200, 200));
    assert_eq!(unauthenticated.status, 426);
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn a_request_without_the_header_passes_only_with_a_browser_session_cookie() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let addr = server.public.addr;
    let cookie = format!("{}={MEMBER_TOKEN}", crate::harness::SESSION_COOKIE);

    // Act
    let browser = get(addr, PROTECTED, &[("cookie", cookie.as_str())]).await;
    let native = get_as(addr, PROTECTED, None, MEMBER_TOKEN).await;
    let anonymous = get(addr, PROTECTED, &[]).await;

    // Assert
    assert_eq!(browser.status, 200);
    // Contract open point 3: the status for a missing header is not specified; 400 like a malformed value.
    assert_eq!(native.status, 400);
    assert_eq!(native.json()["code"], "invalid_request");
    assert_eq!(anonymous.status, 400);
    assert_eq!(server.authenticator.calls().len(), 1);
    server.stop().await;
}

#[tokio::test]
async fn a_bearer_wins_over_a_cookie_when_the_header_is_missing() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let cookie = format!("{}={MEMBER_TOKEN}", crate::harness::SESSION_COOKIE);
    let bearer = format!("Bearer {MEMBER_TOKEN}");

    // Act
    let both = get(
        server.public.addr,
        PROTECTED,
        &[
            ("cookie", cookie.as_str()),
            ("authorization", bearer.as_str()),
        ],
    )
    .await;

    // Assert
    assert_eq!(both.status, 400);
    server.stop().await;
}

#[tokio::test]
async fn malformed_headers_are_400_invalid_request_for_every_value_and_for_a_repeated_header() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let addr = server.public.addr;
    let values = ["foo", "web/x.y", "web/1.2", "tv/1.2.3", "web/1.2.3-rc1", ""];

    // Act
    let mut statuses = Vec::new();
    for value in values {
        let answer = get_as(addr, PROTECTED, Some(value), MEMBER_TOKEN).await;
        assert_eq!(answer.json()["code"], "invalid_request", "{value}");
        statuses.push(answer.status);
    }
    let repeated = get(
        addr,
        PROTECTED,
        &[
            ("x-sunnycloud-client", "web/1.2.0"),
            ("x-sunnycloud-client", "web/1.2.0"),
        ],
    )
    .await;

    // Assert
    assert_eq!(statuses, vec![400; values.len()]);
    assert_eq!(repeated.status, 400);
    assert!(server.authenticator.calls().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn the_counter_counts_supported_and_unsupported_requests_per_family() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let addr = server.public.addr;

    // Act
    for _ in 0..3 {
        get_as(addr, PROTECTED, Some("android/1.2.0"), MEMBER_TOKEN).await;
    }
    for _ in 0..2 {
        get_as(addr, PROTECTED, Some("android/1.1.9"), MEMBER_TOKEN).await;
    }
    get_as(addr, PROTECTED, None, MEMBER_TOKEN).await;
    get_as(addr, PROTECTED, Some("foo"), MEMBER_TOKEN).await;
    get(addr, PUBLIC, &[("x-sunnycloud-client", "android/1.0.0")]).await;

    // Assert
    assert_eq!(metric(&server.metrics, SUPPORTED_ANDROID).await, Some(3.0));
    assert_eq!(
        metric(&server.metrics, UNSUPPORTED_ANDROID).await,
        Some(2.0)
    );
    server.stop().await;
}

#[tokio::test]
async fn every_family_and_support_series_starts_at_zero() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;

    // Act
    let series = metric(
        &server.metrics,
        "sunnycloud_sync_client_versions_total{client=\"ios\",supported=\"no\"}",
    )
    .await;

    // Assert
    assert_eq!(series, Some(0.0));
    server.stop().await;
}

#[tokio::test]
async fn without_a_minimum_every_valid_client_is_supported_and_a_missing_header_passes() {
    // Arrange
    let server = Gated::start(crate::harness::default_config()).await;
    let addr = server.public.addr;

    // Act
    let old = get_as(addr, PROTECTED, Some("android/0.0.1"), MEMBER_TOKEN).await;
    let absent = get_as(addr, PROTECTED, None, MEMBER_TOKEN).await;
    let malformed = get_as(addr, PROTECTED, Some("foo"), MEMBER_TOKEN).await;

    // Assert
    assert_eq!(
        (old.status, absent.status, malformed.status),
        (200, 200, 400)
    );
    assert_eq!(metric(&server.metrics, SUPPORTED_ANDROID).await, Some(1.0));
    server.stop().await;
}

#[tokio::test]
async fn every_class_except_public_is_gated_including_worker_and_share_grant() {
    // Arrange
    let server = Gated::start(gated_config("1.2.0")).await;
    let old = [("x-sunnycloud-client", "android/1.1.9")];

    // Act
    let mut statuses = Vec::new();
    for (class, path) in PROBES {
        let addr = if class == AuthClass::Worker {
            server.internal.addr
        } else {
            server.public.addr
        };
        statuses.push((class, get(addr, path, &old).await.status));
    }

    // Assert
    for (class, status) in statuses {
        let expected = if class == AuthClass::Public { 200 } else { 426 };
        assert_eq!(status, expected, "{class:?} at {}", path_of(class));
    }
    assert!(server.worker_auth.calls().is_empty());
    server.stop().await;
}
