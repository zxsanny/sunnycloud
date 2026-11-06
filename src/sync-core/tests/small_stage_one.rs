//! `stage_small` as a public call of `SyncApi` (AZ-3963): one request per file, typed answer and errors.

mod batch_support;
mod connect_support;
mod request_support;
mod support;
mod tree_support;

use batch_support::{RecordingSink, SpyStore, Timeline, files, id};
use bytes::Bytes;
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, Route, Script};
use sync_core::{BatchRunError, SyncApi, SyncError};
use sync_core_proto::{Staged, hash_block};
use tree_support::{connected, seen};

#[tokio::test]
async fn stage_small_puts_the_body_with_its_block_hash_and_returns_the_staged_row() {
    // Arrange
    let body = Bytes::from_static(b"twelve bytes");
    let hash = hash_block(&body).expect("a small body is one block");
    let answer = Answer::json(200, &json!({ "staged_key": id(7), "size": 12 }));
    let (api, fixture) = connected(Script::new().always(Route::StagedUpload, answer)).await;

    // Act
    let staged = api.stage_small(id(7), body.clone(), hash).await;

    // Assert
    assert_eq!(
        staged,
        Ok(Staged {
            staged_key: id(7),
            size: 12
        })
    );
    let requests = seen(&fixture, Route::StagedUpload);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].target,
        format!("/api/v1/staged-uploads/{}", id(7))
    );
    assert_eq!(
        requests[0].header("x-blake3"),
        Some(hash.to_string().as_str())
    );
    assert_eq!(requests[0].body, body);
}

#[tokio::test]
async fn stage_small_reports_the_staged_cap_as_its_typed_error() {
    // Arrange
    let script = Script::new().always(
        Route::StagedUpload,
        Answer::problem(ErrorCode::StagedCapReached),
    );
    let (api, _fixture) = connected(script).await;
    let body = Bytes::from_static(b"x");
    let hash = hash_block(&body).expect("one block");

    // Act
    let result = api.stage_small(id(1), body, hash).await;

    // Assert
    assert_eq!(result, Err(SyncError::StagedCapReached));
}

#[tokio::test]
async fn stage_small_with_an_unreadable_answer_is_a_protocol_violation() {
    // Arrange
    let script = Script::new().always(
        Route::StagedUpload,
        Answer::json(200, &json!({ "ok": true })),
    );
    let (api, _fixture) = connected(script).await;
    let body = Bytes::from_static(b"x");
    let hash = hash_block(&body).expect("one block");

    // Act
    let result = api.stage_small(id(1), body, hash).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
}

#[tokio::test]
async fn the_executor_refuses_a_staged_row_that_is_not_the_one_it_sent() {
    // Arrange
    let answer = Answer::json(200, &json!({ "staged_key": id(99), "size": 64 }));
    let (api, _fixture) = connected(Script::new().always(Route::StagedUpload, answer)).await;
    let timeline = Timeline::default();

    // Act
    let result = api
        .upload_small(
            files(1, 1, 64),
            SpyStore::new(&timeline),
            RecordingSink::new(&timeline),
        )
        .await;

    // Assert
    assert!(
        matches!(
            result,
            Err(BatchRunError::Sync(SyncError::ProtocolViolation { .. }))
        ),
        "{result:?}"
    );
}
