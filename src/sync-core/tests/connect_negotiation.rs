//! AC-1 to AC-3 of AZ-3959: the scheme check before any socket, the negotiation table and hostile `ServerInfo` numbers.

mod connect_support;
mod request_support;
mod support;

use std::time::{Duration, Instant};

use bytes::Bytes;
use connect_support::{
    CLIENT_VERSION, client, config, connect, connect_to_info, failure, info_script, with,
};
use serde_json::json;
use support::{Answer, Fixture, Route, Script};
use sync_core::{ConnectError, HttpSyncClient};

const WITHIN: Duration = Duration::from_secs(2);

fn mismatch(what: &'static str) -> ConnectError {
    ConnectError::ProtocolMismatch { what }
}

// The release build lacks `allow-plain-http`, and so does every default test build; the feature itself is
// refused at compile time in a release profile (`lib.rs`) and kept out of release trees by `release_isolation`.
#[cfg(not(feature = "allow-plain-http"))]
#[tokio::test]
async fn plain_http_is_refused_before_any_socket_is_opened() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("loopback listener");
    listener.set_nonblocking(true).expect("non-blocking");
    let port = listener.local_addr().expect("address").port();

    let literal = HttpSyncClient::connect(
        config(&format!("http://127.0.0.1:{port}")),
        connect_support::shared_credentials(),
    )
    .await;
    let named = HttpSyncClient::connect(
        config("http://sunnycloud.test"),
        connect_support::shared_credentials(),
    )
    .await;

    assert_eq!(failure(literal), ConnectError::InsecureScheme);
    assert_eq!(failure(named), ConnectError::InsecureScheme);
    let accepted = listener.accept();
    assert_eq!(
        accepted.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::WouldBlock),
        "a connection reached the listener"
    );
}

#[tokio::test]
async fn a_malformed_url_or_identity_is_an_invalid_config() {
    let credentials = connect_support::shared_credentials;
    let mut bad_family = config("https://sunnycloud.test");
    bad_family.client_family = "web".to_owned();
    let mut bad_version = config("https://sunnycloud.test");
    bad_version.client_version = "1.2".to_owned();

    let url = HttpSyncClient::connect(config("ftp://sunnycloud.test"), credentials()).await;
    let family = HttpSyncClient::connect(bad_family, credentials()).await;
    let version = HttpSyncClient::connect(bad_version, credentials()).await;

    assert_eq!(
        failure(url),
        ConnectError::InvalidConfig {
            field: "server_url"
        }
    );
    assert_eq!(
        failure(family),
        ConnectError::InvalidConfig {
            field: "client_family"
        }
    );
    assert_eq!(
        failure(version),
        ConnectError::InvalidConfig {
            field: "client_version"
        }
    );
}

#[tokio::test]
async fn negotiation_decides_each_server_info_case() {
    let upgrade = |result| match failure(result) {
        ConnectError::UpgradeRequired {
            message,
            update_url,
        } => (message, update_url),
        other => panic!("expected UpgradeRequired, got {other:?}"),
    };

    let one_mib = connect_to_info(&with("block_size", json!(1_048_576))).await;
    let too_new = connect_to_info(&with("min_client_version", json!("1.2.4"))).await;
    let no_v1 = connect_to_info(&with("api_versions", json!(["v2"]))).await;
    let two_lanes = connect_to_info(&with("max_lanes", json!(2))).await;
    let sixteen_lanes = connect_to_info(&with("max_lanes", json!(16))).await;

    assert_eq!(failure(one_mib), mismatch("block_size"));
    let (message, update_url) = upgrade(too_new);
    assert!(message.contains("upgrade") && message.contains("1.2.4"));
    assert_eq!(update_url, None);
    let (message, _) = upgrade(no_v1);
    assert!(message.contains("upgrade"));
    let (two, info) = client(two_lanes);
    assert_eq!(two.limits().lanes, 2);
    assert_eq!(info.max_lanes, 2, "the answer is returned as sent");
    assert_eq!(client(sixteen_lanes).0.limits().lanes, 4);
}

#[tokio::test]
async fn the_client_version_equal_to_the_minimum_connects() {
    let equal = connect_to_info(&with("min_client_version", json!(CLIENT_VERSION))).await;

    let (client, info) = client(equal);

    assert_eq!(info.min_client_version, CLIENT_VERSION);
    let limits = client.limits();
    assert_eq!(limits.batch_max_entries, 500);
    assert_eq!(limits.max_staged_per_account, 1000);
}

#[tokio::test]
async fn the_minimum_can_be_given_per_family() {
    let own_family_too_new = connect_to_info(&with(
        "min_client_version",
        json!("android=1.0.0, desktop=1.3.0"),
    ))
    .await;
    let own_family_ok = connect_to_info(&with(
        "min_client_version",
        json!("android=9.0.0,desktop=1.2.0"),
    ))
    .await;
    let family_not_named =
        connect_to_info(&with("min_client_version", json!("android=9.0.0"))).await;
    let malformed = connect_to_info(&with("min_client_version", json!("desktop=latest"))).await;
    let not_a_version = connect_to_info(&with("min_client_version", json!("v1.2.3"))).await;

    assert!(matches!(
        failure(own_family_too_new),
        ConnectError::UpgradeRequired { .. }
    ));
    own_family_ok.expect("own family minimum is met");
    family_not_named.expect("a family without a minimum connects");
    assert_eq!(failure(malformed), mismatch("min_client_version"));
    assert_eq!(failure(not_a_version), mismatch("min_client_version"));
}

#[tokio::test]
async fn the_long_poll_wait_is_the_lower_of_local_and_server_values() {
    let fixture = Fixture::start(info_script(&with("longpoll_max_wait", json!(30)))).await;
    let mut local_longer = config(&fixture.url());
    local_longer.longpoll_wait = Duration::from_secs(40);
    let mut local_shorter = config(&fixture.url());
    local_shorter.longpoll_wait = Duration::from_secs(10);

    let (longer, _) = client(connect(&fixture, local_longer).await);
    let (shorter, _) = client(connect(&fixture, local_shorter).await);

    assert_eq!(longer.limits().longpoll_wait, Duration::from_secs(30));
    assert_eq!(shorter.limits().longpoll_wait, Duration::from_secs(10));
}

#[tokio::test]
async fn hostile_numbers_end_in_a_typed_error_within_two_seconds() {
    let cases = [
        (
            "small_file_max",
            json!(1u64 << 63),
            mismatch("small_file_max"),
        ),
        ("block_size", json!(1u64 << 63), mismatch("block_size")),
        ("batch_max_entries", json!(0), mismatch("batch_max_entries")),
        (
            "batch_max_entries",
            json!(u32::MAX),
            mismatch("batch_max_entries"),
        ),
        ("max_lanes", json!(0), mismatch("max_lanes")),
        (
            "max_staged_per_account",
            json!(0),
            mismatch("max_staged_per_account"),
        ),
        ("longpoll_max_wait", json!(-1), mismatch("server_info")),
        ("longpoll_max_wait", json!(0), mismatch("longpoll_max_wait")),
        (
            "longpoll_max_wait",
            json!(1u64 << 63),
            mismatch("longpoll_max_wait"),
        ),
        ("max_lanes", json!("many"), mismatch("server_info")),
    ];

    for (field, value, expected) in cases {
        let started = Instant::now();
        let result = connect_to_info(&with(field, value.clone())).await;
        assert!(started.elapsed() < WITHIN, "{field}={value} took too long");
        assert_eq!(failure(result), expected, "{field}={value}");
    }
}

#[tokio::test]
async fn a_huge_lane_count_is_clamped_not_refused() {
    let result = connect_to_info(&with("max_lanes", json!(u32::MAX))).await;

    assert_eq!(client(result).0.limits().lanes, 4);
}

#[tokio::test]
async fn an_oversized_or_unreadable_body_ends_in_a_typed_error() {
    let started = Instant::now();
    let huge = Script::new().always(
        Route::ServerInfo,
        Answer::bytes(200, "application/json", Bytes::from(vec![b' '; 17 << 20])),
    );
    let not_json = Script::new().always(
        Route::ServerInfo,
        Answer::bytes(200, "application/json", Bytes::from_static(b"<html>")),
    );
    let huge_fixture = Fixture::start(huge).await;
    let junk_fixture = Fixture::start(not_json).await;

    let huge_result = connect(&huge_fixture, config(&huge_fixture.url())).await;
    let junk_result = connect(&junk_fixture, config(&junk_fixture.url())).await;

    assert_eq!(failure(huge_result), ConnectError::Unreachable);
    assert_eq!(failure(junk_result), mismatch("server_info"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn refusals_of_server_info_map_to_connect_errors() {
    let unauthorized = Script::new().always(
        Route::ServerInfo,
        Answer::problem(sc_error::ErrorCode::Unauthenticated),
    );
    let bare_426 = Script::new().always(Route::ServerInfo, Answer::status(426));
    let not_found = Script::new().always(Route::ServerInfo, Answer::status(404));
    let throttled = Script::new().always(
        Route::ServerInfo,
        Answer::problem(sc_error::ErrorCode::RateLimited).retry_after(1),
    );
    let mut results = Vec::new();
    for script in [unauthorized, bare_426, not_found, throttled] {
        let fixture = Fixture::start(script).await;
        results.push(failure(connect(&fixture, config(&fixture.url())).await));
    }

    assert_eq!(results[0], ConnectError::Unauthorized);
    assert!(matches!(results[1], ConnectError::UpgradeRequired { .. }));
    assert_eq!(results[2], mismatch("server_info"));
    assert_eq!(results[3], ConnectError::Unreachable);
}
