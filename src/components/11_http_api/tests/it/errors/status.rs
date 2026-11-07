use axum::response::IntoResponse;
use http_body_util::BodyExt;
use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use sunnycloud_http_api::ApiProblem;

use super::cases::CASES;
use super::probe::{ErrorProbe, fresh_id};

#[tokio::test]
async fn every_stub_route_answers_its_status_with_a_problem_body_bound_to_the_request_id() {
    // Arrange
    let probe = ErrorProbe::start().await;

    for case in CASES {
        let id = fresh_id();

        // Act
        let reply = probe.get(&format!("/probe/errors/{}", case.name), id).await;

        // Assert
        let body = reply.json();
        assert_eq!(reply.status.as_u16(), case.status, "{}", case.name);
        assert_eq!(reply.header("content-type"), "application/problem+json");
        assert_eq!(reply.header("x-content-type-options"), "nosniff");
        assert_eq!(reply.header("cache-control"), "no-store");
        assert_eq!(body["code"], case.code, "{}", case.name);
        assert_eq!(body["type"], format!("urn:sunnycloud:error:{}", case.code));
        assert_eq!(body["status"], case.status);
        assert_eq!(body["request_id"], id.to_string());
        assert_eq!(reply.header("x-request-id"), id.to_string());
        assert!(
            body["title"]
                .as_str()
                .is_some_and(|title| !title.is_empty())
        );
    }
    probe.stop().await;
}

#[tokio::test]
async fn need_blocks_and_edited_since_are_409_and_batch_too_large_is_422() {
    // Arrange
    let probe = ErrorProbe::start().await;

    // Act
    let need_blocks = probe.get("/probe/errors/need_blocks", fresh_id()).await;
    let edited_since = probe.get("/probe/errors/edited_since", fresh_id()).await;
    let batch_too_large = probe.get("/probe/errors/batch_too_large", fresh_id()).await;

    // Assert
    assert_eq!(need_blocks.status.as_u16(), 409);
    assert_eq!(edited_since.status.as_u16(), 409);
    assert_eq!(batch_too_large.status.as_u16(), 422);
    probe.stop().await;
}

#[tokio::test]
async fn every_code_of_the_table_is_answered_with_its_table_status() {
    for code in ErrorCode::ALL.iter().copied() {
        // Arrange
        let error = DomainError::Business(BusinessError::new(code, BusinessParams::default()));
        let id = fresh_id();

        // Act
        let response = ApiProblem::new(error, id).into_response();

        // Assert
        assert_eq!(
            response.status().as_u16(),
            code.status(),
            "{}",
            code.as_str()
        );
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["status"], code.status());
        assert_eq!(body["code"], code.as_str());
    }
}
