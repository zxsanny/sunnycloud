//! AC-1, AC-2 and AC-5 of AZ-3963: records are saved before staging, a killed run is replayed with the same
//! `commit_id`, and a member the server lost is staged again from its source.

mod batch_support;
mod connect_support;
mod request_support;
mod support;

use std::sync::Arc;
use std::time::Duration;

use batch_support::{
    Event, Fake, FakeConfig, Gate, RecordingSink, SpyStore, Timeline, files, resolver, restart,
    start,
};
use sc_clock::SystemClock;
use support::Fixture;
use sync_core::{BatchOutcome, CheckpointStore, SmallFile, SyncError, new_batch};
use sync_core_proto::EntryResult;

#[derive(Clone, Copy)]
enum Kill {
    SaveBatch,
    Stagings(usize),
    CommitSent,
}

struct Killed {
    fake: Arc<Fake>,
    fixture: Fixture,
    timeline: Timeline,
    store: Arc<SpyStore>,
    files: Vec<SmallFile>,
    staged_at_kill: usize,
    reports_at_kill: usize,
}

/// Runs 300 files until `kill`, then drops the client and the run, which is all a killed process leaves.
async fn run_until_killed(kill: Kill) -> Killed {
    let timeline = Timeline::default();
    let (fake, fixture, client) = start(FakeConfig::default(), &timeline).await;
    let files = files(1, 300, 256);
    let sink = RecordingSink::new(&timeline);
    let (reached, store, release) = match kill {
        Kill::SaveBatch => {
            let (gate, release) = Gate::new();
            let reached = Arc::clone(&gate.reached);
            let store = SpyStore::gated_on_first_save(&timeline, gate);
            (reached, store, Some(release))
        }
        Kill::Stagings(count) => (
            fake.notify_after_stagings(count),
            SpyStore::new(&timeline),
            None,
        ),
        Kill::CommitSent => (
            fake.notify_after_commit(Duration::from_millis(500)),
            SpyStore::new(&timeline),
            None,
        ),
    };

    tokio::select! {
        result = client.upload_small(files.clone(), store.clone(), sink.clone()) => {
            panic!("the run ended before the kill point: {result:?}");
        }
        () = reached.notified() => {}
    }
    let staged_at_kill = fake.staged_count();
    let reports_at_kill = sink.reports().len();
    if let Some(release) = release {
        release.send(()).expect("the gated store is waiting");
    }
    drop(client);
    Killed {
        fake,
        fixture,
        timeline,
        store,
        files,
        staged_at_kill,
        reports_at_kill,
    }
}

async fn replay_after(kill: Kill) -> (Killed, Arc<RecordingSink>) {
    // Arrange
    let killed = run_until_killed(kill).await;
    let records = killed.store.load_batches().expect("store reads");
    assert_eq!(records.len(), 1, "the unfinished record survives the kill");
    assert_eq!(records[0].entries.len(), 300);
    let client = restart(&killed.fixture).await;
    let sink = RecordingSink::new(&killed.timeline);

    // Act
    let skipped = client
        .resume_batches(killed.store.clone(), sink.clone(), &resolver(&killed.files))
        .await
        .expect("the replay finishes");

    // Assert
    assert!(skipped.is_empty());
    let reports = sink.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].commit_id, records[0].commit_id);
    assert_eq!(reports[0].entries, records[0].entries);
    let BatchOutcome::Committed(result) = &reports[0].outcome else {
        panic!("the replay commits: {:?}", reports[0].outcome);
    };
    assert_eq!(result.entries.len(), 300);
    assert!(
        result
            .entries
            .iter()
            .all(|entry| matches!(entry, EntryResult::Committed { .. }))
    );
    assert_eq!(killed.fake.file_count(), 300);
    assert!(killed.fake.duplicates().is_empty());
    assert!(killed.store.load_batches().expect("store reads").is_empty());
    let id = records[0].commit_id;
    let report_at = killed.timeline.position(&Event::Report(id));
    let clear_at = killed.timeline.position(&Event::Clear(id));
    assert!(report_at < clear_at, "clear_batch runs after the hand-over");
    assert_eq!(killed.store.saves(), 1, "the replay keeps the saved record");
    (killed, sink)
}

#[tokio::test]
async fn save_batch_is_recorded_before_every_staging_of_its_members() {
    // Arrange
    let timeline = Timeline::default();
    let config = FakeConfig {
        batch_max_entries: 100,
        ..FakeConfig::default()
    };
    let (fake, _fixture, client) = start(config, &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);

    // Act
    let skipped = client
        .upload_small(files(1, 300, 512), store.clone(), sink.clone())
        .await
        .expect("the run finishes");

    // Assert
    assert!(skipped.is_empty());
    let reports = sink.reports();
    assert_eq!(reports.len(), 3);
    let events = timeline.events();
    for report in &reports {
        let saved = timeline
            .position(&Event::Save(report.commit_id))
            .expect("every batch is saved");
        for entry in &report.entries {
            let staged = events
                .iter()
                .position(|event| *event == Event::Stage(entry.staged_key))
                .expect("every member is staged");
            assert!(
                saved < staged,
                "save_batch precedes the staging of a member"
            );
        }
    }
    assert_eq!(store.saves(), 3);
    assert_eq!(fake.file_count(), 300);
    assert_eq!(fake.bad_hash(), 0, "X-Blake3 is the hash of the body");
    assert!(store.load_batches().expect("store reads").is_empty());
}

#[tokio::test]
async fn a_run_killed_after_save_batch_replays_the_same_batch() {
    // Arrange, Act, Assert
    let (killed, _) = replay_after(Kill::SaveBatch).await;
    assert_eq!(killed.staged_at_kill, 0, "nothing was staged at the kill");
    assert_eq!(killed.fake.applied(), 1);
    assert_eq!(killed.fake.replayed(), 0);
}

#[tokio::test]
async fn a_run_killed_after_150_stagings_replays_the_same_batch() {
    // Arrange, Act, Assert
    let (killed, _) = replay_after(Kill::Stagings(150)).await;
    assert!((150..300).contains(&killed.staged_at_kill));
    assert_eq!(killed.fake.applied(), 1);
    assert_eq!(killed.fake.replayed(), 0);
}

#[tokio::test]
async fn a_run_killed_after_the_commit_was_sent_gets_the_stored_response() {
    // Arrange, Act, Assert
    let (killed, _) = replay_after(Kill::CommitSent).await;
    assert_eq!(killed.reports_at_kill, 0, "the host never got the results");
    assert_eq!(killed.fake.applied(), 1, "the batch is applied once");
    assert_eq!(
        killed.fake.replayed(),
        1,
        "the repeated id gets the stored response"
    );
}

#[tokio::test]
async fn a_member_the_server_lost_is_staged_again_before_the_commit() {
    // Arrange
    let timeline = Timeline::default();
    let (fake, _fixture, client) = start(FakeConfig::default(), &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);
    let files = files(1, 20, 128);
    let record = new_batch(
        &SystemClock::new(),
        files.iter().map(|file| file.entry.clone()).collect(),
    )
    .expect("a fresh batch id");
    store.save_batch(record.clone()).expect("store writes");

    // Act
    let skipped = client
        .resume_batches(store.clone(), sink.clone(), &resolver(&files))
        .await
        .expect("the replay finishes");

    // Assert
    assert!(skipped.is_empty());
    assert_eq!(fake.stage_requests(), 20);
    assert_eq!(fake.file_count(), 20);
    let events = timeline.events();
    let commit_at = timeline
        .position(&Event::Commit(record.commit_id))
        .expect("the batch is committed");
    let last_stage = events
        .iter()
        .rposition(|event| matches!(event, Event::Stage(_)))
        .expect("members are staged");
    assert!(
        last_stage < commit_at,
        "every member is staged before the commit"
    );
}

#[tokio::test]
async fn a_member_whose_source_changed_is_not_staged_and_leaves_the_batch() {
    // Arrange
    let timeline = Timeline::default();
    let (fake, _fixture, client) = start(FakeConfig::default(), &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);
    let files = files(1, 10, 128);
    let record = new_batch(
        &SystemClock::new(),
        files.iter().map(|file| file.entry.clone()).collect(),
    )
    .expect("a fresh batch id");
    store.save_batch(record.clone()).expect("store writes");
    let edited = files[3].clone();
    let replacement = support::seeded_bytes(999, 128);
    let source = batch_support::MemSource::new(replacement);
    let mut replayed = files.clone();
    replayed[3] = SmallFile {
        entry: edited.entry.clone(),
        source,
    };

    // Act
    let skipped = client
        .resume_batches(store.clone(), sink.clone(), &resolver(&replayed))
        .await
        .expect("the replay finishes");

    // Assert
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].entry, edited.entry);
    assert_eq!(skipped[0].error, SyncError::SourceChanged);
    assert_eq!(fake.stage_requests(), 9);
    assert!(
        !timeline
            .events()
            .contains(&Event::Stage(edited.entry.staged_key))
    );
    let reports = sink.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].entries.len(), 9);
    assert!(store.load_batches().expect("store reads").is_empty());
}

#[tokio::test]
async fn files_from_zero_bytes_to_2_mib_take_the_staged_path() {
    // Arrange
    let timeline = Timeline::default();
    let (fake, _fixture, client) = start(FakeConfig::default(), &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);
    let mut all = files(1, 1, 0);
    all.extend(files(2, 1, 2 * 1024 * 1024 - 1));
    all.extend(files(3, 1, 2 * 1024 * 1024));

    // Act
    let skipped = client
        .upload_small(all, store, sink)
        .await
        .expect("the run finishes");

    // Assert
    assert!(skipped.is_empty());
    assert_eq!(fake.file_count(), 3);
    assert_eq!(fake.bad_hash(), 0);
}

#[tokio::test]
async fn a_file_that_grew_beyond_the_staged_limit_is_left_out() {
    // Arrange
    let timeline = Timeline::default();
    let (fake, _fixture, client) = start(FakeConfig::default(), &timeline).await;
    let store = SpyStore::new(&timeline);
    let sink = RecordingSink::new(&timeline);
    let mut all = files(1, 2, 64);
    all[1].source = batch_support::MemSource::new(support::seeded_bytes(1, 2 * 1024 * 1024 + 1));

    // Act
    let skipped = client
        .upload_small(all, store, sink)
        .await
        .expect("the run finishes");

    // Assert
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].error, SyncError::SourceChanged);
    assert_eq!(fake.file_count(), 1);
}
