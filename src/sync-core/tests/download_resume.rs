//! `download` under broken streams (AZ-3964 AC-3, AC-4): a reset costs at most the block in flight, a kill is
//! continued from the last verified block, and an early end is never success. The 2,560-block file of the
//! spec is scaled to 16 blocks: the property is per block, not per file size.

mod connect_support;
mod download_support;
mod request_support;
mod support;
mod tree_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use download_support::{BLOCK, SpySink, range_start, ranges, reference, revision_script, view_of};
use support::{Answer, Fault, Route, Script, seeded_bytes};
use sync_core::{SyncApi, SyncError, Verified};
use sync_core_proto::RevisionView;
use tree_support::connected;

const BLOCKS: usize = 16;
const BROKEN: usize = BLOCKS / 2;
const BROKEN_START: u64 = (BROKEN * BLOCK) as u64;
/// Sends per request (`max_attempts_per_block` of the test config).
const ATTEMPTS: usize = 3;

fn file() -> Bytes {
    seeded_bytes(60, BLOCKS * BLOCK)
}

/// Serves `data` with ranges; `fault` decides per request, from its range start, whether the stream breaks.
fn serving(
    data: &Bytes,
    view: &RevisionView,
    fault: impl Fn(u64) -> Option<Fault> + Send + Sync + 'static,
) -> Script {
    let data = data.clone();
    revision_script(view).respond(Route::RevisionContent, move |request| {
        let start = range_start(request.header("range").unwrap_or_default());
        let answer = Answer::content(data.clone());
        match fault(start) {
            Some(fault) => answer.fault(fault),
            None => answer,
        }
    })
}

#[tokio::test]
async fn a_reset_in_the_middle_of_a_block_rereads_that_block_only() {
    // Arrange
    let data = file();
    let view = view_of(&data);
    let armed = AtomicBool::new(true);
    let script = serving(&data, &view, move |start| {
        (start == BROKEN_START && armed.swap(false, Ordering::SeqCst))
            .then_some(Fault::ResetAfterBytes(BLOCK / 2))
    });
    let (api, fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert_eq!(
        result,
        Ok(Verified {
            file_hash: view.content_hash
        })
    );
    let sent = ranges(&fixture);
    assert_eq!(sent.len(), BLOCKS + 1, "one block asked for twice, no more");
    let after_reset: Vec<u64> = sent[BROKEN + 1..]
        .iter()
        .map(|range| range_start(range))
        .collect();
    assert_eq!(
        after_reset[0], BROKEN_START,
        "the retry starts at the broken block"
    );
    assert_eq!(
        sink.offsets(),
        SpySink::block_offsets(0, BLOCKS),
        "each block reaches the sink once"
    );
    assert_eq!(sink.content(), data.to_vec());
    assert_eq!(sink.finishes(), vec![true]);
}

#[tokio::test]
async fn a_download_that_dies_in_block_8_continues_from_the_last_verified_block() {
    // Arrange
    let data = file();
    let view = view_of(&data);
    let broken = Arc::new(AtomicBool::new(true));
    let still_broken = Arc::clone(&broken);
    let script = serving(&data, &view, move |start| {
        (start == BROKEN_START && still_broken.load(Ordering::SeqCst))
            .then_some(Fault::ResetAfterBytes(BLOCK / 2))
    });
    let (api, fixture) = connected(script).await;
    let sink = SpySink::new();
    let killed = api.download(&reference(&view), sink.clone(), 0).await;
    let kept = sink.written_len();
    broken.store(false, Ordering::SeqCst);

    // Act
    let resumed = api.download(&reference(&view), sink.clone(), kept).await;

    // Assert
    assert!(matches!(killed, Err(SyncError::Transport(_))), "{killed:?}");
    assert_eq!(
        kept, BROKEN_START,
        "blocks 0 to 7 were verified and written"
    );
    assert_eq!(
        resumed,
        Ok(Verified {
            file_hash: view.content_hash
        })
    );
    let sent = ranges(&fixture);
    assert_eq!(range_start(&sent[BROKEN + ATTEMPTS]), BROKEN_START);
    assert_eq!(sent.len(), BROKEN + ATTEMPTS + (BLOCKS - BROKEN));
    assert_eq!(sink.offsets(), SpySink::block_offsets(0, BLOCKS));
    assert_eq!(sink.content(), data.to_vec());
    assert_eq!(sink.finishes(), vec![false, true]);
}

#[tokio::test]
async fn a_stream_that_ends_early_is_never_verified() {
    for fault in [
        Fault::DropAfterBytes(BLOCK / 2),
        Fault::ResetAfterBytes(BLOCK - 1),
        Fault::DropConnection,
        Fault::ResetStream,
    ] {
        // Arrange
        let data = file();
        let view = view_of(&data);
        let script = serving(&data, &view, move |start| {
            (start == BROKEN_START).then_some(fault)
        });
        let (api, _fixture) = connected(script).await;
        let sink = SpySink::new();

        // Act
        let result = api.download(&reference(&view), sink.clone(), 0).await;

        // Assert
        assert!(
            matches!(result, Err(SyncError::Transport(_))),
            "{fault:?}: {result:?}"
        );
        assert_eq!(
            sink.offsets(),
            SpySink::block_offsets(0, BROKEN),
            "{fault:?}"
        );
        assert!(!sink.finishes().contains(&true), "{fault:?}");
    }
}
