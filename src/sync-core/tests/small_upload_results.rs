//! AC-6 of AZ-3963: batch errors and per-entry outcomes are typed, and the executor reports them to the host.

mod batch_support;
mod connect_support;
mod request_support;
mod support;
mod tree_support;

use batch_support::{RecordingSink, SpyStore, Timeline, files, id};
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, LoggedRequest, Route, Script};
use sync_core::{BatchOutcome, BatchRunError, CheckpointStore, SyncApi, SyncError};
use sync_core_proto::{BatchCommit, BatchEntry, BatchResult, EntryResult};
use tree_support::{body_json, connected, file_hash, seen};

fn entry(n: u128) -> BatchEntry {
    BatchEntry {
        staged_key: id(n),
        parent: id(1),
        name: format!("f{n}"),
        base_revision: None,
        file_hash: file_hash(n as u64),
        client_meta: None,
        import_hint: None,
    }
}

/// The staged row the server holds for the request: its key from the route, the byte count of the body.
fn echo_staged(request: &LoggedRequest) -> Answer {
    let key = request.target.rsplit('/').next().expect("a route segment");
    Answer::json(
        200,
        &json!({ "staged_key": key, "size": request.body.len() }),
    )
}

fn committed(n: u128) -> EntryResult {
    EntryResult::Committed {
        node_id: id(0x100 + n),
        revision_id: id(0x200 + n),
        conflict_copy: None,
    }
}

#[tokio::test]
async fn batch_commit_posts_the_batch_and_returns_committed_and_entry_error_per_entry() {
    // Arrange
    let answer = BatchResult {
        entries: vec![
            committed(1),
            EntryResult::EntryError {
                code: "invalid_name".to_owned(),
            },
            committed(3),
        ],
    };
    let script = Script::new().on(Route::BatchCommit, Answer::json(200, &answer));
    let (api, fixture) = connected(script).await;
    let commit = BatchCommit {
        commit_id: id(0xC0),
        entries: vec![entry(1), entry(2), entry(3)],
    };

    // Act
    let result = api.batch_commit(commit).await;

    // Assert
    assert_eq!(result, Ok(answer));
    let requests = seen(&fixture, Route::BatchCommit);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, "/api/v1/commits:batch");
    let body = body_json(&requests[0]);
    assert_eq!(body["commit_id"], json!(id(0xC0)));
    assert_eq!(body["entries"].as_array().map(Vec::len), Some(3));
}

#[tokio::test]
async fn batch_commit_reports_a_reused_id_and_an_oversized_batch_as_typed_errors() {
    // Arrange
    let script = Script::new()
        .on(
            Route::BatchCommit,
            Answer::problem(ErrorCode::CommitIdReused),
        )
        .on(
            Route::BatchCommit,
            Answer::problem(ErrorCode::BatchTooLarge),
        );
    let (api, _fixture) = connected(script).await;
    let commit = BatchCommit {
        commit_id: id(0xC0),
        entries: vec![entry(1)],
    };

    // Act
    let reused = api.batch_commit(commit.clone()).await;
    let too_large = api.batch_commit(commit).await;

    // Assert
    assert_eq!(reused, Err(SyncError::CommitIdReused));
    assert_eq!(too_large, Err(SyncError::BatchTooLarge));
}

#[tokio::test]
async fn the_executor_hands_a_reused_id_to_the_host_and_then_clears_the_record() {
    // Arrange
    let script = Script::new().respond(Route::StagedUpload, echo_staged).on(
        Route::BatchCommit,
        Answer::problem(ErrorCode::CommitIdReused),
    );
    let (api, _fixture) = connected(script).await;
    let timeline = Timeline::default();
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let skipped = api
        .upload_small(files(1, 3, 64), store.clone(), sink.clone())
        .await
        .expect("a reused id is a result, not a failure of the run");

    // Assert
    assert!(skipped.is_empty());
    let reports = sink.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, BatchOutcome::IdReused);
    assert!(store.load_batches().expect("store reads").is_empty());
}

#[tokio::test]
async fn the_executor_reports_the_failed_entry_next_to_the_committed_ones() {
    // Arrange
    let answer = BatchResult {
        entries: vec![
            committed(1),
            EntryResult::EntryError {
                code: "upload_not_found".to_owned(),
            },
            committed(3),
        ],
    };
    let script = Script::new()
        .respond(Route::StagedUpload, echo_staged)
        .on(Route::BatchCommit, Answer::json(200, &answer));
    let (api, _fixture) = connected(script).await;
    let timeline = Timeline::default();
    let sink = RecordingSink::new(&timeline);

    // Act
    api.upload_small(files(1, 3, 64), SpyStore::new(&timeline), sink.clone())
        .await
        .expect("the run finishes");

    // Assert
    let reports = sink.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, BatchOutcome::Committed(answer));
}

#[tokio::test]
async fn a_result_of_the_wrong_length_keeps_the_record_for_a_replay() {
    // Arrange
    let answer = BatchResult {
        entries: vec![committed(1)],
    };
    let script = Script::new()
        .respond(Route::StagedUpload, echo_staged)
        .on(Route::BatchCommit, Answer::json(200, &answer));
    let (api, _fixture) = connected(script).await;
    let timeline = Timeline::default();
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let result = api
        .upload_small(files(1, 3, 64), store.clone(), sink.clone())
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(BatchRunError::Sync(SyncError::ProtocolViolation { .. }))
    ));
    assert!(sink.reports().is_empty());
    assert_eq!(store.load_batches().expect("store reads").len(), 1);
}

#[tokio::test]
async fn a_quota_refusal_stops_the_run_and_keeps_the_record() {
    // Arrange
    let script = Script::new().always(
        Route::StagedUpload,
        Answer::problem(ErrorCode::QuotaExceeded),
    );
    let (api, _fixture) = connected(script).await;
    let timeline = Timeline::default();
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let result = api.upload_small(files(1, 3, 64), store.clone(), sink).await;

    // Assert
    assert!(matches!(
        result,
        Err(BatchRunError::Sync(SyncError::QuotaExceeded))
    ));
    assert_eq!(store.load_batches().expect("store reads").len(), 1);
}
