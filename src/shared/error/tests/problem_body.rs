#![cfg(feature = "server")]

use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, FieldProblem,
    ProblemBody, ResyncReason,
};
use serde_json::Value;
use std::io;
use std::time::Duration;
use uuid::Uuid;

fn request_id() -> CorrelationId {
    CorrelationId::new(Uuid::from_u128(0x0192_8f3a_7b00_7abc_8def_0123_4567_89ab))
}

fn body_bytes(body: &ProblemBody) -> Vec<u8> {
    serde_json::to_vec(body).expect("problem body serialises")
}

fn json(body: &ProblemBody) -> Value {
    serde_json::from_slice(&body_bytes(body)).expect("problem body is JSON")
}

#[test]
fn not_found_bodies_are_byte_identical_for_all_five_causes() {
    // missing id, cross-account id, missing / expired / revoked share link
    let causes: Vec<DomainError> = (0..5).map(|_| BusinessError::not_found().into()).collect();

    let bodies: Vec<Vec<u8>> = causes
        .iter()
        .map(|cause| body_bytes(&cause.problem(request_id())))
        .collect();

    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
    let value: Value = serde_json::from_slice(&bodies[0]).expect("JSON");
    assert_eq!(value["code"], "not_found");
    assert_eq!(value["status"], 404);
    assert_eq!(value["type"], "urn:sunnycloud:error:not_found");
    assert_eq!(value["request_id"], request_id().to_string());
}

#[test]
fn unexpected_error_serialises_to_internal_without_planted_strings() {
    let secrets = [
        "hunter2-s3cr3t",
        "SELECT * FROM accounts",
        "/var/lib/sunnycloud/blobs/ab",
    ];
    let planted = io::Error::other(secrets.join(" | "));
    let error = DomainError::unexpected(planted, request_id());

    let bytes = body_bytes(&error.problem(request_id()));

    let text = String::from_utf8(bytes).expect("UTF-8");
    for secret in secrets {
        assert!(!text.contains(secret), "leaked {secret}");
    }
    let value: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(value["code"], "internal");
    assert_eq!(value["status"], 500);
    assert_eq!(value["request_id"], request_id().to_string());
    let keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["code", "request_id", "status", "title", "type"].map(String::from)
    );
}

#[test]
fn unexpected_display_and_code_do_not_name_the_source() {
    let error = DomainError::unexpected(io::Error::other("db password=abc"), request_id());

    assert_eq!(error.code(), ErrorCode::Internal);
    assert!(!error.to_string().contains("db password"));
}

#[test]
fn retry_after_is_serialised_in_whole_seconds_rounded_up() {
    let params = BusinessParams {
        retry_after: Some(Duration::from_millis(1500)),
        ..BusinessParams::default()
    };
    let error = DomainError::Business(BusinessError::new(ErrorCode::RateLimited, params));

    let value = json(&error.problem(request_id()));

    assert_eq!(value["retry_after"], 2);
    assert_eq!(value["status"], 429);
}

#[test]
fn parameters_appear_only_when_set() {
    let params = BusinessParams {
        need_blocks: vec![3, 9],
        reason: Some(ResyncReason::BelowPurgeHorizon),
        min_version: Some("1.4.0".to_owned()),
        ..BusinessParams::default()
    };
    let with = json(
        &DomainError::Business(BusinessError::new(ErrorCode::ResyncRequired, params))
            .problem(request_id()),
    );
    let without = json(
        &DomainError::Business(BusinessError::new(
            ErrorCode::NameConflict,
            BusinessParams::default(),
        ))
        .problem(request_id()),
    );

    assert_eq!(with["need_blocks"], serde_json::json!([3, 9]));
    assert_eq!(with["reason"], "below_purge_horizon");
    assert_eq!(with["min_version"], "1.4.0");
    for key in [
        "need_blocks",
        "reason",
        "min_version",
        "retry_after",
        "problems",
    ] {
        assert!(without.get(key).is_none(), "{key} must be absent");
    }
}

#[test]
fn validation_lists_field_problems_under_invalid_request() {
    let error = DomainError::Validation(vec![FieldProblem {
        field: "name".to_owned(),
        problem: "too_long".to_owned(),
    }]);

    let value = json(&error.problem(request_id()));

    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert_eq!(value["status"], 400);
    assert_eq!(value["problems"][0]["field"], "name");
    assert_eq!(value["problems"][0]["problem"], "too_long");
}
