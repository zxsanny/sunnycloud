use std::time::Duration;

use hyper::StatusCode;
use sc_observability::NoLabels;

use super::client::H2;
use super::stack::{SERVER_INFO, Stack};

#[tokio::test]
async fn a_panic_is_a_500_problem_and_the_h2_connection_serves_the_next_request() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;
    let before = stack.http_metrics.panics.get(NoLabels);

    // Act
    let panicked = connection.get("/probe/panic").await;
    let next = connection.get(SERVER_INFO).await;

    // Assert
    assert_eq!(panicked.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(panicked.header("content-type"), "application/problem+json");
    let body = panicked.json();
    assert_eq!(body["code"], "internal");
    assert_eq!(body["request_id"], panicked.request_id());
    assert!(!panicked.text().contains("secret-payload"));
    assert_eq!(panicked.header("x-content-type-options"), "nosniff");
    assert_eq!(
        panicked.header("strict-transport-security"),
        "max-age=31536000"
    );
    assert_eq!(next.status, StatusCode::OK);
    assert_eq!(next.header("x-content-type-options"), "nosniff");
    assert_eq!(stack.http_metrics.panics.get(NoLabels), before + 1);
    stack.stop().await;
}

#[tokio::test]
async fn a_panic_does_not_disturb_a_stream_in_flight_on_the_same_connection() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;
    let mut other = connection.clone();
    let slow = tokio::spawn(async move { other.get("/probe/slow").await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Act
    let panicked = connection.get("/probe/panic").await;
    let slow = slow.await.expect("the slow request task");

    // Assert
    assert_eq!(panicked.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(slow.status, StatusCode::OK);
    stack.stop().await;
}

#[tokio::test]
async fn every_panic_counts_once_and_the_family_is_exported_at_zero() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;
    let idle = stack.series("sunnycloud_http_panics_total").await;

    // Act
    for _ in 0..3 {
        connection.get("/probe/panic").await;
    }

    // Assert
    assert_eq!(idle, ["sunnycloud_http_panics_total 0"]);
    assert_eq!(
        stack.series("sunnycloud_http_panics_total").await,
        ["sunnycloud_http_panics_total 3"]
    );
    stack.stop().await;
}
