//! AC-6 to AC-9 of AZ-3962: a lost commit answer is recovered by `start_upload` with the identical request
//! (`sync_api.md` § Lost-commit recovery), conflict outcomes carry the server's choice, a changed source stops
//! before the commit and a blocklist mismatch re-hashes under the same key.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;
mod upload_support;

use std::sync::atomic::{AtomicU32, Ordering};

use hash_support::{BLOCK, Content, GeneratedSource, KEY, Progress, SpyStore};
use sc_error::ErrorCode;
use serde_json::Value;
use support::{Answer, Fault, LoggedRequest, Route, Script};
use sync_core::{CheckpointStore, FileIdentity, SyncError, UploadState};
use sync_core_proto::CommitOutcome;
use tree_support::seen;
use upload_support::{
    block_indexes, client_keys, conflict_copy, connected, every_block, file, hashed, key, node,
    nothing_to_send, published, request, session, start_bodies, store,
};

/// Index of the first request on `route` in the log, which counts every request.
fn first_seq(requests: &[LoggedRequest], route: Route) -> u64 {
    requests
        .iter()
        .find(|request| request.route == Some(route))
        .map(|request| request.seq)
        .expect("the route was called")
}

/// The commit applies on the server and its answer never arrives; `start_upload` then answers with the
/// revision that commit made, as 03 does for a key whose session committed within `upload_session_ttl`.
fn lost_commit_script(revision: uuid::Uuid) -> Script {
    Script::new()
        .on(Route::StartUpload, session(&every_block(2), 4))
        .on(Route::StartUpload, nothing_to_send(revision))
        .always(Route::PutBlock, Answer::no_content())
        .on(
            Route::Commit,
            published(node(0x100), revision).fault(Fault::DropConnection),
        )
}

#[tokio::test]
async fn a_lost_commit_answer_for_an_existing_node_ends_in_nothing_to_send_with_no_block_sent_again()
 {
    // Arrange
    let (source, hashed) = file(2, Content::Pattern);
    let (api, fixture) = connected(lost_commit_script(node(0x201))).await;
    let checkpoints = SpyStore::new();

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), Some(node(0x200))),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::NothingToSend {
            revision_id: node(0x201)
        })
    );
    let bodies = start_bodies(&fixture);
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0], bodies[1], "the identical request is sent again");
    assert_eq!(bodies[0]["base_revision"], serde_json::json!(node(0x200)));
    let requests = fixture.requests();
    let commit_at = first_seq(&requests, Route::Commit);
    assert!(
        requests
            .iter()
            .filter(|request| request.route == Some(Route::PutBlock))
            .all(|request| request.seq < commit_at),
        "no block byte after the commit"
    );
    let states: Vec<UploadState> = checkpoints
        .saved()
        .into_iter()
        .map(|(_, checkpoint)| checkpoint.state)
        .collect();
    assert_eq!(
        states,
        vec![UploadState::Uploading, UploadState::Committing]
    );
    assert_eq!(checkpoints.load(KEY), Ok(None));
}

#[tokio::test]
async fn a_lost_commit_answer_for_a_new_file_ends_in_its_one_revision_without_a_conflict_copy() {
    // Arrange
    let (source, hashed) = file(2, Content::Pattern);
    let (api, fixture) = connected(lost_commit_script(node(0x201))).await;

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), None),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::NothingToSend {
            revision_id: node(0x201)
        })
    );
    assert_eq!(seen(&fixture, Route::Commit).len(), 1);
    assert_eq!(block_indexes(&fixture.requests()).len(), 2);
    let bodies = start_bodies(&fixture);
    assert_eq!(bodies[0], bodies[1]);
    assert_eq!(bodies[1]["base_revision"], Value::Null);
}

#[tokio::test]
async fn a_typed_error_from_the_recovery_is_returned_and_the_checkpoint_kept() {
    // Arrange
    let (source, hashed) = file(1, Content::Pattern);
    let script = Script::new()
        .on(Route::StartUpload, session(&[0], 4))
        .on(
            Route::StartUpload,
            Answer::problem(ErrorCode::ParentNotFound),
        )
        .always(Route::PutBlock, Answer::no_content())
        .on(Route::Commit, Answer::problem(ErrorCode::UploadSessionGone));
    let (api, fixture) = connected(script).await;
    let checkpoints = store();

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), None),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(outcome, Err(SyncError::ParentNotFound));
    assert_eq!(seen(&fixture, Route::StartUpload).len(), 2);
    let kept = checkpoints.load(KEY).expect("the store reads");
    assert_eq!(
        kept.map(|checkpoint| (checkpoint.commit_id, checkpoint.state)),
        Some((key(1), UploadState::Committing))
    );
}

#[tokio::test]
async fn a_stale_base_commit_returns_the_server_conflict_copy() {
    // Arrange
    let (source, hashed) = file(1, Content::Pattern);
    let script = Script::new()
        .on(Route::StartUpload, session(&[0], 4))
        .always(Route::PutBlock, Answer::no_content())
        .on(
            Route::Commit,
            conflict_copy(node(0x100), node(0x202), node(0x101)),
        );
    let (api, fixture) = connected(script).await;

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), Some(node(0x200))),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::ConflictCopy {
            node_id: node(0x100),
            copy_node_id: node(0x101)
        })
    );
    assert_eq!(seen(&fixture, Route::Commit).len(), 1);
}

#[tokio::test]
async fn a_taken_name_returns_the_server_named_copy_after_exactly_one_commit_request() {
    // Arrange
    let (source, hashed) = file(1, Content::Pattern);
    let script = Script::new()
        .on(Route::StartUpload, session(&[0], 4))
        .always(Route::PutBlock, Answer::no_content())
        .on(
            Route::Commit,
            conflict_copy(node(0x100), node(0x202), node(0x100)),
        );
    let (api, fixture) = connected(script).await;

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), None),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::ConflictCopy {
            node_id: node(0x100),
            copy_node_id: node(0x100)
        })
    );
    assert_eq!(seen(&fixture, Route::Commit).len(), 1);
    assert_eq!(seen(&fixture, Route::StartUpload).len(), 1);
}

#[tokio::test]
async fn a_source_whose_identity_changed_before_the_commit_ends_in_source_changed_without_a_commit()
{
    // Arrange
    let len = 3 * BLOCK;
    let changed = FileIdentity {
        size: len,
        mtime_ns: 1_800_000_000_000_000_000,
    };
    let source = std::sync::Arc::new(GeneratedSource::new(len).changing_after(3, changed));
    let script = Script::new()
        .on(Route::StartUpload, session(&every_block(3), 4))
        .always(Route::PutBlock, Answer::no_content());
    let (api, fixture) = connected(script).await;

    // Act
    let outcome = api
        .upload(
            request(source, hashed(len, Content::Pattern), key(1), None),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(outcome, Err(SyncError::SourceChanged));
    assert!(seen(&fixture, Route::Commit).is_empty());
}

#[tokio::test]
async fn a_blocklist_mismatch_rehashes_the_file_and_restarts_with_the_same_key() {
    // Arrange
    let (source, hashed) = file(3, Content::Pattern);
    let starts = AtomicU32::new(0);
    let script = Script::new()
        .respond(Route::StartUpload, move |_| {
            match starts.fetch_add(1, Ordering::SeqCst) {
                0 => session(&every_block(3), 4),
                _ => session(&[], 4),
            }
        })
        .always(Route::PutBlock, Answer::no_content())
        .on(Route::Commit, Answer::problem(ErrorCode::BlocklistMismatch))
        .on(Route::Commit, published(node(0x100), node(0x200)));
    let (api, fixture) = connected(script).await;

    // Act
    let outcome = api
        .upload(
            request(source.clone(), hashed, key(1), None),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert!(
        matches!(outcome, Ok(CommitOutcome::Published { .. })),
        "{outcome:?}"
    );
    let keys = client_keys(&fixture);
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], keys[1]);
    assert_eq!(seen(&fixture, Route::Commit).len(), 2);
    let block_reads = |offset: u64| {
        source
            .read_offsets()
            .into_iter()
            .filter(|read| *read == offset)
            .count()
    };
    assert_eq!(block_reads(0), 2, "sent once, hashed again once");
    assert_eq!(block_indexes(&fixture.requests()).len(), 3);
}
