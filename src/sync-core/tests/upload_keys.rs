//! AC-4 and AC-5 of AZ-3962: retries carry the stored `client_key`, new content a new one; `NothingToSend`
//! ends an upload with no block sent.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;
mod upload_support;

use std::sync::{Arc, Mutex, PoisonError};

use hash_support::{Content, KEY, Progress, SpyStore};
use sc_error::ErrorCode;
use serde_json::{Value, json};
use support::{Answer, Route, Script};
use sync_core::{CheckpointStore, SyncError, UploadState};
use sync_core_proto::CommitOutcome;
use upload_support::{
    block_indexes, client_keys, connected, file, key, node, nothing_to_send, request, session,
    start_bodies, store,
};

#[tokio::test]
async fn three_attempts_carry_one_uuid_v7_key_and_new_content_gets_a_new_one() {
    // Arrange
    let (source, hashed) = file(2, Content::Pattern);
    let (edited, edited_hash) = file(3, Content::Pattern);
    let script = Script::new()
        .always(Route::StartUpload, session(&[0], 4))
        .always(Route::PutBlock, Answer::problem(ErrorCode::QuotaExceeded));
    let (api, fixture) = connected(script).await;
    let checkpoints = store();

    // Act
    let mut attempts = Vec::new();
    for n in 1..=3 {
        let attempt = request(source.clone(), hashed.clone(), key(n), None);
        attempts.push(
            api.upload(attempt, checkpoints.clone(), Progress::new())
                .await,
        );
    }
    let new_content = api
        .upload(
            request(edited, edited_hash, key(4), None),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(attempts, vec![Err(SyncError::QuotaExceeded); 3]);
    assert_eq!(new_content, Err(SyncError::QuotaExceeded));
    let keys = client_keys(&fixture);
    assert_eq!(
        keys,
        vec![json!(key(1)), json!(key(1)), json!(key(1)), json!(key(4))]
    );
    assert_eq!(key(1).get_version_num(), 7);
    let kept = checkpoints.load(KEY).expect("the store reads");
    assert_eq!(kept.map(|checkpoint| checkpoint.commit_id), Some(key(4)));
}

#[tokio::test]
async fn nothing_to_send_returns_the_revision_sends_no_block_and_clears_the_checkpoint() {
    // Arrange
    let (source, hashed) = file(3, Content::Pattern);
    let script = Script::new().on(Route::StartUpload, nothing_to_send(node(0x300)));
    let (api, fixture) = connected(script).await;
    let checkpoints = SpyStore::new();

    // Act
    let outcome = api
        .upload(
            request(source, hashed.clone(), key(1), None),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::NothingToSend {
            revision_id: node(0x300)
        })
    );
    assert!(block_indexes(&fixture.requests()).is_empty());
    assert_eq!(checkpoints.load(KEY), Ok(None));
    let body = &start_bodies(&fixture)[0];
    assert_eq!(
        body["content_hash"],
        Value::from(hashed.file_hash.to_string())
    );
    assert_eq!(body["declared_size"], json!(3 * hash_support::BLOCK));
}

#[tokio::test]
async fn the_checkpoint_holds_the_session_and_key_before_the_first_block() {
    // Arrange
    let (source, hashed) = file(1, Content::Pattern);
    let checkpoints = SpyStore::new();
    let saves_at_put = Arc::new(Mutex::new(Vec::new()));
    let (spy, seen_saves) = (checkpoints.clone(), Arc::clone(&saves_at_put));
    let script = Script::new()
        .on(Route::StartUpload, session(&[0], 4))
        .respond(Route::PutBlock, move |_| {
            seen_saves
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(spy.saved().len());
            Answer::problem(ErrorCode::QuotaExceeded)
        });
    let (api, _fixture) = connected(script).await;

    // Act
    let outcome = api
        .upload(
            request(source, hashed.clone(), key(1), None),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;

    // Assert
    assert_eq!(outcome, Err(SyncError::QuotaExceeded));
    let saved: Vec<_> = checkpoints
        .saved()
        .into_iter()
        .map(|(_, checkpoint)| checkpoint)
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].state, UploadState::Uploading);
    assert_eq!(saved[0].session_id, Some(upload_support::UPLOAD));
    assert_eq!(saved[0].commit_id, key(1));
    assert_eq!(saved[0].file_hash, Some(hashed.file_hash));
    let at_put = saves_at_put
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(
        at_put,
        vec![1],
        "the checkpoint was saved before the block arrived"
    );
}
