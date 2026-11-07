use std::time::Duration;

use axum::response::IntoResponse;
use contracts::{ChangeFeedError, ResyncReason, SignInError};
use http_body_util::BodyExt;
use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use serde_json::Value;
use sunnycloud_http_api::ApiProblem;

use super::probe::{ErrorProbe, fresh_id};

async fn answer(error: DomainError) -> (axum::http::HeaderMap, Value) {
    let response = ApiProblem::new(error, fresh_id()).into_response();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (headers, serde_json::from_slice(&bytes).expect("json"))
}

fn limited(code: ErrorCode, delay: Duration) -> DomainError {
    let params = BusinessParams {
        retry_after: Some(delay),
        ..BusinessParams::default()
    };
    DomainError::Business(BusinessError::new(code, params))
}

#[tokio::test]
async fn rate_limited_and_service_unavailable_carry_retry_after_equal_to_the_body() {
    // Arrange
    let probe = ErrorProbe::start().await;

    // Act
    let rate_limited = probe.get("/probe/errors/rate_limited", fresh_id()).await;
    let unavailable = probe
        .get("/probe/errors/service_unavailable", fresh_id())
        .await;

    // Assert
    assert_eq!(rate_limited.status.as_u16(), 429);
    assert_eq!(rate_limited.header("retry-after"), "17");
    assert_eq!(rate_limited.json()["retry_after"], 17);
    assert_eq!(unavailable.status.as_u16(), 503);
    assert_eq!(unavailable.header("retry-after"), "9");
    assert_eq!(unavailable.json()["retry_after"], 9);
    probe.stop().await;
}

#[tokio::test]
async fn too_many_holds_is_a_429_rate_limited_with_retry_after() {
    // Arrange
    let probe = ErrorProbe::start().await;

    // Act
    let reply = probe.get("/probe/errors/too_many_holds", fresh_id()).await;

    // Assert
    assert_eq!(reply.status.as_u16(), 429);
    assert_eq!(reply.json()["code"], "rate_limited");
    assert_eq!(reply.header("retry-after"), "5");
    assert_eq!(reply.json()["retry_after"], 5);
    probe.stop().await;
}

#[tokio::test]
async fn need_blocks_lists_the_missing_indexes_and_resync_required_carries_its_reason() {
    // Arrange
    let probe = ErrorProbe::start().await;

    // Act
    let need_blocks = probe.get("/probe/errors/need_blocks", fresh_id()).await;
    let resync = probe.get("/probe/errors/resync_required", fresh_id()).await;

    // Assert
    assert_eq!(need_blocks.json()["need_blocks"], serde_json::json!([3, 7]));
    assert_eq!(resync.status.as_u16(), 410);
    assert_eq!(resync.json()["reason"], "epoch_changed");
    probe.stop().await;
}

#[tokio::test]
async fn upgrade_required_carries_min_version() {
    // Arrange
    let probe = ErrorProbe::start().await;

    // Act
    let reply = probe
        .get("/probe/errors/upgrade_required", fresh_id())
        .await;

    // Assert
    assert_eq!(reply.status.as_u16(), 426);
    assert_eq!(reply.json()["min_version"], "1.2.0");
    probe.stop().await;
}

#[tokio::test]
async fn every_resync_reason_reaches_the_body() {
    for (reason, wire) in [
        (ResyncReason::EpochChanged, "epoch_changed"),
        (ResyncReason::BelowPurgeHorizon, "below_purge_horizon"),
        (ResyncReason::CursorAhead, "cursor_ahead"),
    ] {
        // Arrange
        let error = ChangeFeedError::ResyncRequired { reason };
        let id = fresh_id();

        // Act
        let response = ApiProblem::from_port(error, id).into_response();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();

        // Assert
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["reason"], wire);
    }
}

#[tokio::test]
async fn a_sub_second_retry_after_is_rounded_up_in_header_and_body() {
    // Arrange
    let error = limited(ErrorCode::RateLimited, Duration::from_millis(1_200));

    // Act
    let (headers, body) = answer(error).await;

    // Assert
    assert_eq!(headers["retry-after"], "2");
    assert_eq!(body["retry_after"], 2);
}

#[tokio::test]
async fn a_503_without_retry_after_has_neither_header_nor_field() {
    // Arrange
    let error = DomainError::from(BusinessError::new(
        ErrorCode::ServiceUnavailable,
        BusinessParams::default(),
    ));

    // Act
    let (headers, body) = answer(error).await;

    // Assert
    assert!(headers.get("retry-after").is_none());
    assert!(body.get("retry_after").is_none());
}

#[tokio::test]
async fn retry_after_of_a_sign_in_rate_limit_is_taken_from_the_port_error() {
    // Arrange
    let error = SignInError::RateLimited {
        retry_after: Duration::from_secs(30),
    };

    // Act
    let response = ApiProblem::from_port(error, fresh_id()).into_response();

    // Assert
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(response.headers()["retry-after"], "30");
}
