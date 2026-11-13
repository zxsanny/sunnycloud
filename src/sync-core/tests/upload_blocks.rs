//! AC-1, AC-2 and AC-3 of AZ-3962: a block is done only on 204, a rejected block is sent again, at most four
//! blocks are in flight, and a resume sends only the blocks the session still lacks under the same key.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;
mod upload_support;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use hash_support::{BLOCK, Content, Progress};
use request_support::eventually;
use sc_error::ErrorCode;
use serde_json::{Value, json};
use support::{Answer, Fault, Route, Script};
use sync_core::SyncError;
use sync_core_proto::CommitOutcome;
use tree_support::{body_json, seen};
use upload_support::{
    block_indexes, client_keys, connected, connected_with, every_block, file, hex, key, node,
    published, request, sends_of, session, store,
};

#[tokio::test]
async fn non_204_answers_leave_the_block_needed_and_progress_counts_only_the_204() {
    // Arrange
    let (source, hashed) = file(1, Content::Pattern);
    let script = Script::new()
        .on(Route::StartUpload, session(&[0], 4))
        .on(
            Route::PutBlock,
            Answer::json(200, &json!({ "stored": true })),
        )
        .on(Route::PutBlock, Answer::status(202))
        .on(Route::PutBlock, Answer::status(500))
        .on(
            Route::PutBlock,
            Answer::no_content().fault(Fault::ResetStream),
        )
        .always(Route::PutBlock, Answer::no_content())
        .on(Route::Commit, published(node(0x100), node(0x200)));
    let (api, fixture) = connected_with(script, |config| sync_core::SyncConfig {
        max_attempts_per_block: 8,
        ..config
    })
    .await;
    let progress = Progress::new();

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), None),
            store(),
            progress.clone(),
        )
        .await;

    // Assert
    assert_eq!(
        outcome,
        Ok(CommitOutcome::Published {
            node_id: node(0x100),
            revision_id: node(0x200)
        })
    );
    assert_eq!(block_indexes(&fixture.requests()), vec![0; 5]);
    assert_eq!(progress.calls(), vec![(BLOCK, BLOCK)]);
}

#[tokio::test]
async fn a_block_rejected_for_its_hash_is_sent_once_more_and_the_commit_carries_the_client_hashes()
{
    // Arrange
    let (source, hashed) = file(5, Content::Pattern);
    let rejected = AtomicBool::new(false);
    let script = Script::new()
        .on(Route::StartUpload, session(&every_block(5), 4))
        .respond(Route::PutBlock, move |sent| {
            if sent.block_index() == Some(3) && !rejected.swap(true, Ordering::SeqCst) {
                Answer::problem(ErrorCode::BlockHashMismatch)
            } else {
                Answer::no_content()
            }
        })
        .on(Route::Commit, published(node(0x100), node(0x200)));
    let (api, fixture) = connected(script).await;
    let expected: Vec<Value> = hashed.block_hashes.iter().map(hex).collect();

    // Act
    let outcome = api
        .upload(
            request(source, hashed, key(1), None),
            store(),
            Progress::new(),
        )
        .await;

    // Assert
    assert!(matches!(outcome, Ok(CommitOutcome::Published { .. })));
    let sends: Vec<usize> = (0..5).map(|index| sends_of(&fixture, index)).collect();
    assert_eq!(sends, vec![1, 1, 1, 2, 1]);
    let commits = seen(&fixture, Route::Commit);
    assert_eq!(commits.len(), 1);
    assert_eq!(body_json(&commits[0])["blocklist"], Value::from(expected));
    let put = seen(&fixture, Route::PutBlock);
    assert!(
        put.iter()
            .all(|request| request.header("x-blake3").is_some())
    );
}

/// The fake account of the resume test: it acks blocks until 37 are stored, then drops the connection of
/// every block `PUT` until the session is asked for again.
#[derive(Default)]
struct Dropping {
    acked: BTreeSet<u32>,
    resumed: bool,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resume_after_a_drop_at_37_of_100_sends_only_the_missing_blocks_on_four_lanes_and_one_key()
 {
    // Arrange
    let (source, hashed) = file(100, Content::BlockFill);
    let state = Arc::new(Mutex::new(Dropping::default()));
    let (on_start, on_put) = (Arc::clone(&state), Arc::clone(&state));
    let script = Script::new()
        .respond(Route::StartUpload, move |_| {
            let mut state = on_start.lock().unwrap_or_else(PoisonError::into_inner);
            let missing: Vec<u32> = every_block(100)
                .into_iter()
                .filter(|index| !state.acked.contains(index))
                .collect();
            state.resumed = !state.acked.is_empty();
            session(&missing, 4)
        })
        .respond(Route::PutBlock, move |sent| {
            let mut state = on_put.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.resumed && state.acked.len() >= 37 {
                return Answer::no_content().fault(Fault::DropConnection);
            }
            state.acked.extend(sent.block_index());
            Answer::no_content()
        })
        .on(Route::Commit, published(node(0x100), node(0x200)));
    let (api, fixture) = connected(script).await;
    let checkpoints = store();

    // Act
    let dropped = api
        .upload(
            request(source.clone(), hashed.clone(), key(1), None),
            checkpoints.clone(),
            Progress::new(),
        )
        .await;
    // Requests the first run abandoned may still be read by the fake; the second run starts once they ended.
    eventually(5_000, "the first run's requests to end", || {
        fixture.with_stats(|stats| stats.block_puts_in_flight) == 0
    })
    .await;
    let first_run = fixture.requests().len();
    let acked_before: BTreeSet<u32> = state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .acked
        .clone();
    let resumed = api
        .upload(
            request(source, hashed, key(2), None),
            checkpoints,
            Progress::new(),
        )
        .await;

    // Assert
    assert!(matches!(dropped, Err(SyncError::Transport(_))));
    assert!(matches!(resumed, Ok(CommitOutcome::Published { .. })));
    assert_eq!(acked_before.len(), 37);
    let resent: BTreeSet<u32> = block_indexes(&fixture.requests()[first_run..])
        .into_iter()
        .collect();
    let expected: BTreeSet<u32> = every_block(100)
        .into_iter()
        .filter(|index| !acked_before.contains(index))
        .collect();
    assert_eq!(resent, expected, "only the blocks the session lacks");
    assert_eq!(client_keys(&fixture), vec![json!(key(1)), json!(key(1))]);
    assert!(fixture.with_stats(|stats| stats.max_block_puts_in_flight) <= 4);
}

#[tokio::test]
async fn a_session_lane_limit_of_two_holds_two_blocks_in_flight() {
    // Arrange
    let (source, hashed) = file(8, Content::BlockFill);
    let script = Script::new()
        .on(Route::StartUpload, session(&every_block(8), 2))
        .always(Route::PutBlock, Answer::no_content())
        .on(Route::Commit, published(node(0x100), node(0x200)));
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
    assert!(matches!(outcome, Ok(CommitOutcome::Published { .. })));
    assert!(fixture.with_stats(|stats| stats.max_block_puts_in_flight) <= 2);
    assert_eq!(block_indexes(&fixture.requests()).len(), 8);
}
