//! AC-4 of AZ-3963: batches stay within `batch_max_entries`, this client stays under half of the staged cap,
//! and a `StagedCapReached` is answered by committing first.

mod batch_support;
mod connect_support;
mod request_support;
mod support;

use batch_support::{FakeConfig, RecordingSink, SpyStore, Timeline, files, start};
use support::Route;
use sync_core::{BatchRunError, CheckpointStore, SyncError};

const CONFIG: FakeConfig = FakeConfig {
    batch_max_entries: 100,
    max_staged_per_account: 200,
    other_staged: 0,
};

#[tokio::test]
async fn a_thousand_files_commit_in_batches_of_at_most_100_and_stage_at_most_100() {
    // Arrange
    let timeline = Timeline::default();
    let (fake, _fixture, client) = start(CONFIG, &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let skipped = client
        .upload_small(files(1, 1000, 64), store.clone(), sink.clone())
        .await
        .expect("no BatchTooLarge, no cap refusal");

    // Assert
    assert!(skipped.is_empty());
    assert_eq!(fake.file_count(), 1000);
    assert_eq!(fake.max_batch_entries(), 100);
    assert_eq!(fake.max_staged_uncommitted(), 100, "half of the cap of 200");
    assert_eq!(fake.cap_refusals(), 0);
    assert_eq!(sink.reports().len(), 10);
    assert!(store.load_batches().expect("store reads").is_empty());
}

#[tokio::test]
async fn after_a_cap_refusal_the_client_commits_before_it_stages_more() {
    // Arrange
    let timeline = Timeline::default();
    let crowded = FakeConfig {
        other_staged: 150,
        ..CONFIG
    };
    let (fake, fixture, client) = start(crowded, &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let skipped = client
        .upload_small(files(1, 1000, 64), store.clone(), sink.clone())
        .await
        .expect("other devices leave room for 50 files at a time");

    // Assert
    assert!(skipped.is_empty());
    assert_eq!(fake.file_count(), 1000);
    assert!(fake.cap_refusals() > 0, "the cap was reached");
    assert!(fake.max_staged_uncommitted() <= 50);
    assert!(fake.max_batch_entries() <= 100);
    let calls: Vec<(Route, Option<u16>, String)> = fixture
        .requests()
        .into_iter()
        .filter_map(|request| {
            let route = request.route?;
            matches!(route, Route::StagedUpload | Route::BatchCommit)
                .then(|| (route, request.answered, request.target))
        })
        .collect();
    let refused: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, (route, status, _))| *route == Route::StagedUpload && *status == Some(409))
        .map(|(at, _)| at)
        .collect();
    assert!(!refused.is_empty());
    for at in refused {
        let key = &calls[at].2;
        let retried = calls[at + 1..]
            .iter()
            .position(|(route, _, target)| *route == Route::StagedUpload && target == key)
            .map(|offset| at + 1 + offset)
            .expect("a refused file is staged again");
        assert!(
            calls[at..retried]
                .iter()
                .any(|(route, _, _)| *route == Route::BatchCommit),
            "a commit separates a refusal from the next staging of that file"
        );
    }
}

#[tokio::test]
async fn nothing_staged_and_the_cap_full_ends_with_staged_cap_reached_and_no_record() {
    // Arrange
    let timeline = Timeline::default();
    let full = FakeConfig {
        other_staged: 200,
        ..CONFIG
    };
    let (fake, _fixture, client) = start(full, &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let result = client
        .upload_small(files(1, 5, 64), store.clone(), sink.clone())
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(BatchRunError::Sync(SyncError::StagedCapReached))
    ));
    assert_eq!(fake.file_count(), 0);
    assert!(sink.reports().is_empty());
    assert!(store.load_batches().expect("store reads").is_empty());
}
