//! AC-4, AC-5 (the pacing rule) and AC-6 of AZ-3961: a changed source is detected, the CPU share paces the run,
//! and memory stays at one block buffer.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use hash_support::{BLOCK, Content, GeneratedSource, Progress, SpyStore, checkpoint_into};
use support::Script;
use sync_core::{FileIdentity, HttpSyncClient, SyncApi, SyncConfig, SyncError};
use tree_support::connected;

async fn hash(api: &HttpSyncClient, source: Arc<GeneratedSource>) -> Result<(), SyncError> {
    api.hash_file(
        source,
        None,
        Progress::new(),
        checkpoint_into(&SpyStore::new()),
    )
    .await
    .map(|_| ())
}

#[tokio::test]
async fn a_source_whose_mtime_changes_during_hashing_is_a_changed_source_and_gives_no_result() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = GeneratedSource::new(10 * BLOCK);
    let touched = FileIdentity {
        mtime_ns: source.original_identity().mtime_ns + 1,
        ..source.original_identity()
    };
    let source = Arc::new(source.changing_after(5, touched));

    // Act
    let result = api
        .hash_file(
            source,
            None,
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

    // Assert
    assert_eq!(result, Err(SyncError::SourceChanged));
}

#[tokio::test]
async fn a_source_that_grows_during_hashing_is_a_changed_source() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = GeneratedSource::new(4 * BLOCK);
    let grown = FileIdentity {
        size: 4 * BLOCK + 10,
        ..source.original_identity()
    };
    let source = Arc::new(source.changing_after(2, grown));

    // Act
    let result = hash(&api, source).await;

    // Assert
    assert_eq!(result, Err(SyncError::SourceChanged));
}

#[tokio::test]
async fn an_unchanged_identity_is_not_a_changed_source() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = GeneratedSource::new(4 * BLOCK);
    let same = source.original_identity();
    let source = Arc::new(source.changing_after(2, same));

    // Act
    let result = hash(&api, source).await;

    // Assert
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn a_file_on_disk_that_gets_bytes_appended_while_hashing_is_a_changed_source() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let disk = hash_support::DiskSource::create("grows.bin", &hash_support::pattern(3 * BLOCK))
        .growing_after(2, b"more");

    // Act
    let result = api
        .hash_file(
            Arc::new(disk),
            None,
            Progress::new(),
            checkpoint_into(&SpyStore::new()),
        )
        .await;

    // Assert
    assert_eq!(result, Err(SyncError::SourceChanged));
}

#[tokio::test]
async fn a_share_of_five_percent_takes_much_longer_than_a_full_core_for_the_same_blocks() {
    // Arrange
    let blocks = 8;
    let (full_api, _full_fixture) = connected(Script::new()).await;
    let (slow_api, _slow_fixture) = slow_client(5).await;
    // Act
    let at_full = timed(&full_api, blocks).await;
    let at_five_percent = timed(&slow_api, blocks).await;

    // Assert
    assert!(
        at_five_percent >= at_full * 3,
        "5% took {at_five_percent:?}, 100% took {at_full:?}"
    );
}

async fn timed(api: &HttpSyncClient, blocks: u64) -> Duration {
    let source = Arc::new(GeneratedSource::with_content(
        blocks * BLOCK,
        Content::BlockFill,
    ));
    let started = Instant::now();
    hash(api, source).await.expect("a stable source is hashed");
    started.elapsed()
}

async fn slow_client(share: u8) -> (HttpSyncClient, support::Fixture) {
    use connect_support::{client, config, connect, valid_info};
    use support::{Answer, Fixture, Route};
    let script = Script::new().always(Route::ServerInfo, Answer::json(200, &valid_info()));
    let fixture = Fixture::start(script).await;
    let config = SyncConfig {
        hash_cpu_share: share,
        ..config(&fixture.url())
    };
    let (api, _) = client(connect(&fixture, config).await);
    (api, fixture)
}

#[tokio::test]
async fn hashing_64_blocks_holds_one_block_buffer_at_a_time() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = Arc::new(GeneratedSource::with_content(
        64 * BLOCK,
        Content::BlockFill,
    ));

    // Act
    let result = hash(&api, source.clone()).await;

    // Assert
    assert_eq!(result, Ok(()));
    assert_eq!(source.read_offsets().len(), 64);
    assert_eq!(source.most_blocks_alive(), 1);
}

#[tokio::test]
#[ignore = "hashes 5 GiB: about 20 s in a debug build"]
async fn hashing_a_5_gib_source_holds_one_block_buffer_at_a_time() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = Arc::new(GeneratedSource::with_content(
        5 * 1024 * 1024 * 1024,
        Content::BlockFill,
    ));

    // Act
    let result = hash(&api, source.clone()).await;

    // Assert
    assert_eq!(result, Ok(()));
    assert_eq!(source.read_offsets().len(), 2560);
    assert_eq!(source.most_blocks_alive(), 1);
}

#[tokio::test]
async fn cancel_ends_hashing_with_cancelled_before_another_block_is_read() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = Arc::new(GeneratedSource::new(4 * BLOCK));
    api.cancel();

    // Act
    let result = hash(&api, source.clone()).await;

    // Assert
    assert_eq!(result, Err(SyncError::Cancelled));
    assert!(source.read_offsets().is_empty());
}
