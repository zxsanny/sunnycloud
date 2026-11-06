//! AC-2, AC-3 and the Io clause of AZ-3961: resume reads only the blocks after the saved state, the state is
//! handed to the host every 32 blocks, and an I/O error leaves the last handed state valid.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;

use std::sync::Arc;

use hash_support::{BLOCK, GeneratedSource, KEY, Progress, SpyStore, checkpoint_into};
use support::Script;
use sync_core::{SyncApi, SyncError, UploadState};
use sync_core_proto::ResumeHashState;
use tree_support::connected;

const BLOCKS: u64 = 100;

fn hundred_blocks() -> GeneratedSource {
    GeneratedSource::new(BLOCKS * BLOCK)
}

#[tokio::test]
async fn resume_after_64_blocks_reads_only_blocks_64_to_99_and_equals_a_full_hash() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let full = api
        .hash_file(
            Arc::new(hundred_blocks()),
            None,
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("a full hash");
    let state = ResumeHashState::from_hashes(full.block_hashes[..64].to_vec());
    let source = Arc::new(hundred_blocks());

    // Act
    let resumed = api
        .hash_file(
            source.clone(),
            Some(state),
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("a resumed hash");

    // Assert
    let expected_offsets: Vec<u64> = (64..BLOCKS).map(|block| block * BLOCK).collect();
    assert_eq!(source.read_offsets(), expected_offsets);
    assert_eq!(resumed, full);
}

#[tokio::test]
async fn a_run_stopped_after_64_blocks_resumes_from_the_state_it_handed_to_the_host() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let store = SpyStore::new();
    let broken = Arc::new(hundred_blocks().failing_at_read(70));
    let first = api
        .hash_file(broken, None, Progress::new(), checkpoint_into(&store))
        .await;
    let saved = store.saved();
    let last = saved.last().expect("a state was handed over").1.clone();
    let source = Arc::new(hundred_blocks());

    // Act
    let resumed = api
        .hash_file(
            source.clone(),
            Some(ResumeHashState::from_hashes(last.block_hashes.clone())),
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("the resumed hash");

    // Assert
    assert_eq!(
        first,
        Err(SyncError::Io {
            kind: std::io::ErrorKind::PermissionDenied
        })
    );
    assert_eq!(
        last.block_hashes.len(),
        64,
        "the last valid state is block 64"
    );
    assert_eq!(source.read_offsets().len(), 36);
    assert_eq!(resumed.block_hashes[..64], last.block_hashes[..]);
}

#[tokio::test]
async fn the_state_is_handed_to_the_host_after_blocks_32_64_and_96_of_100() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let store = SpyStore::new();

    // Act
    let hashed = api
        .hash_file(
            Arc::new(hundred_blocks()),
            None,
            Progress::new(),
            checkpoint_into(&store),
        )
        .await
        .expect("a full hash");

    // Assert
    let saved = store.saved();
    let sizes: Vec<usize> = saved.iter().map(|(_, c)| c.block_hashes.len()).collect();
    assert_eq!(sizes, vec![32, 64, 96]);
    for (key, checkpoint) in &saved {
        assert_eq!(key, KEY);
        assert_eq!(checkpoint.state, UploadState::Hashing);
        assert_eq!(checkpoint.file_hash, None);
        assert_eq!(checkpoint.session_id, None);
        let n = checkpoint.block_hashes.len();
        assert_eq!(checkpoint.block_hashes[..], hashed.block_hashes[..n]);
    }
}

#[tokio::test]
async fn a_resumed_run_hands_over_at_multiples_of_32_of_the_whole_file() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let store = SpyStore::new();
    let full = api
        .hash_file(
            Arc::new(hundred_blocks()),
            None,
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("a full hash");
    let state = ResumeHashState::from_hashes(full.block_hashes[..40].to_vec());

    // Act
    api.hash_file(
        Arc::new(hundred_blocks()),
        Some(state),
        Progress::new(),
        checkpoint_into(&store),
    )
    .await
    .expect("a resumed hash");

    // Assert
    let sizes: Vec<usize> = store
        .saved()
        .iter()
        .map(|(_, c)| c.block_hashes.len())
        .collect();
    assert_eq!(sizes, vec![64, 96]);
}

#[tokio::test]
async fn a_store_that_fails_does_not_fail_the_hash() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let store = SpyStore::failing();

    // Act
    let result = api
        .hash_file(
            Arc::new(hundred_blocks()),
            None,
            Progress::new(),
            checkpoint_into(&store),
        )
        .await;

    // Assert
    assert_eq!(result.expect("hashing goes on").block_hashes.len(), 100);
    assert_eq!(store.saved().len(), 3, "every hand-over was tried");
}

#[tokio::test]
async fn a_state_with_more_blocks_than_the_file_has_is_a_changed_source() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let full = api
        .hash_file(
            Arc::new(hundred_blocks()),
            None,
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("a full hash");
    let state = ResumeHashState::from_hashes(full.block_hashes[..90].to_vec());
    let shorter = Arc::new(GeneratedSource::new(50 * BLOCK));

    // Act
    let result = api
        .hash_file(
            shorter.clone(),
            Some(state),
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

    // Assert
    assert_eq!(result, Err(SyncError::SourceChanged));
    assert!(shorter.read_offsets().is_empty());
}

#[tokio::test]
async fn a_state_that_does_not_describe_its_own_blocks_is_refused_before_any_read() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = Arc::new(hundred_blocks());
    let state = ResumeHashState {
        blocks_done: 5,
        block_hashes: Vec::new(),
    };

    // Act
    let result = api
        .hash_file(
            source.clone(),
            Some(state),
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

    // Assert
    assert_eq!(
        result,
        Err(SyncError::Io {
            kind: std::io::ErrorKind::InvalidInput
        })
    );
    assert!(source.read_offsets().is_empty());
}
