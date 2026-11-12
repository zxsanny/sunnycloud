//! AZ-3875: the client call of the test-image route `GET /internal/v1/test/clock-offset` (feature `test-clock`).
#![cfg(feature = "test-clock")]

mod api_support;

use std::time::Duration;

use api_support::{MARKER, Peer, Reply, WORKER_ID, client, refused_url};
use sunnycloud_worker::ApiError;

#[tokio::test]
async fn the_offset_is_read_with_the_worker_bearer_and_worker_id() {
    // Arrange
    let peer = Peer::start(|_| Reply::json(200, r#"{"offset_seconds":3600}"#)).await;

    // Act
    let offset = client(&peer.base_url()).clock_offset().await;

    // Assert
    assert_eq!(offset.expect("offset"), Duration::from_secs(3_600));
    let requests = peer.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/internal/v1/test/clock-offset");
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {MARKER}").as_str())
    );
    assert_eq!(requests[0].header("x-worker-id"), Some(WORKER_ID));
}

#[tokio::test]
async fn a_403_is_forbidden() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(403)).await;

    // Act
    let result = client(&peer.base_url()).clock_offset().await;

    // Assert
    assert!(matches!(result, Err(ApiError::Forbidden)), "{result:?}");
}

#[tokio::test]
async fn a_404_is_a_protocol_error_not_an_obsolete_job() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(404)).await;

    // Act
    let result = client(&peer.base_url()).clock_offset().await;

    // Assert
    assert!(matches!(result, Err(ApiError::Protocol(_))), "{result:?}");
}

#[tokio::test]
async fn a_body_without_the_offset_is_a_protocol_error() {
    // Arrange
    let peer = Peer::start(|_| Reply::json(200, r#"{"offset":1}"#)).await;

    // Act
    let result = client(&peer.base_url()).clock_offset().await;

    // Assert
    assert!(matches!(result, Err(ApiError::Protocol(_))), "{result:?}");
}

#[tokio::test]
async fn a_5xx_and_a_refused_connection_are_transient() {
    // Arrange
    let failing = Peer::start(|_| Reply::status(503)).await;
    let refused = refused_url().await;

    // Act
    let from_status = client(&failing.base_url()).clock_offset().await;
    let from_connection = client(&refused).clock_offset().await;

    // Assert
    assert!(
        matches!(from_status, Err(ApiError::Transient(_))),
        "{from_status:?}"
    );
    assert!(
        matches!(from_connection, Err(ApiError::Transient(_))),
        "{from_connection:?}"
    );
}
