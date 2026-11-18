//! AC-4: hashes saved before upload, at most `hash_ahead` of them ahead, none repeated after a restart.

use std::collections::BTreeSet;

use mobile_backup::{BackupOutcome, Trigger};
use sync_core::UploadState;

use crate::support::Rig;

const BLOCK: usize = 2 * 1024 * 1024;

#[tokio::test]
async fn hashes_stay_within_the_window_are_stored_before_their_upload_and_a_restart_hashes_nothing_twice()
 {
    // Arrange: one asset per commit, so the run stops after exactly 20 uploads.
    let mut rig = Rig::new(8, 1);
    for id in 1..=50 {
        rig.source.add(id, 1024);
    }
    let stopper = rig.backup.clone();
    rig.api.on_ack(move |acked| {
        if acked == 20 {
            stopper.cancel();
        }
    });

    // Act
    let stopped = rig.run(Trigger::FirstSync).await;
    let first = {
        let record = rig.api.record();
        (
            record.hashed.clone(),
            record.most_unfinished,
            record.unstored.clone(),
        )
    };
    rig.restart();
    let finished = rig.run(Trigger::FirstSync).await;
    let second = {
        let record = rig.api.record();
        (
            record.hashed.clone(),
            record.most_unfinished,
            record.unstored.clone(),
        )
    };

    // Assert
    assert_eq!(stopped, BackupOutcome::Partial { remaining: 30 });
    assert_eq!(finished, BackupOutcome::Completed);
    assert!(
        first.1 <= 9 && second.1 <= 9,
        "the one uploading and at most 8 ahead: {first:?} {second:?}"
    );
    assert!(
        first.2.is_empty() && second.2.is_empty(),
        "a hash was not stored before its upload"
    );
    let all: Vec<&String> = first.0.iter().chain(second.0.iter()).collect();
    assert_eq!(all.len(), 50, "an asset was hashed twice");
    assert_eq!(all.iter().collect::<BTreeSet<_>>().len(), 50);
    assert_eq!(rig.db.count("uploaded"), 50);
}

#[tokio::test]
async fn a_large_asset_hashed_before_a_stop_keeps_its_block_hashes_and_is_not_hashed_again() {
    // Arrange
    let mut rig = Rig::new(8, 50);
    for id in 1..=3 {
        rig.source.add(id, BLOCK + 1);
    }
    let stopper = rig.backup.clone();
    rig.api.on_ack(move |acked| {
        if acked == 1 {
            stopper.cancel();
        }
    });
    let stopped = rig.run(Trigger::FirstSync).await;
    let hashed_first = rig.api.record().hashed.len();
    let rows = rig.db.session_count();
    rig.restart();

    // Act
    let finished = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(stopped, BackupOutcome::Partial { remaining: 2 });
    assert_eq!(hashed_first, 3);
    assert_eq!(
        rows, 2,
        "a resume record for each hashed asset that did not upload yet"
    );
    assert_eq!(finished, BackupOutcome::Completed);
    let record = rig.api.record();
    assert!(
        record.hashed.is_empty(),
        "restart hashed {:?}",
        record.hashed
    );
    for call in &record.large {
        let session = call
            .session
            .as_ref()
            .expect("the block path starts from its resume record");
        assert_eq!(session.state, UploadState::Uploading);
        assert_eq!(session.block_hashes.len(), 2);
        assert_eq!(session.commit_id, call.key);
    }
    assert_eq!(rig.server.file_count(), 3);
}

#[tokio::test]
async fn a_deadline_that_has_passed_ends_the_run_partial_with_the_assets_still_waiting() {
    // Arrange
    let rig = Rig::new(8, 50);
    for id in 1..=5 {
        rig.source.add(id, 1024);
    }
    let ctx = mobile_backup::RunContext {
        trigger: Trigger::AppOpen,
        deadline: Some(std::time::Instant::now()),
    };

    // Act
    let outcome = rig
        .backup
        .run_backup(ctx, rig.source.clone())
        .await
        .expect("the run ends");

    // Assert
    assert_eq!(outcome, BackupOutcome::Partial { remaining: 5 });
    assert_eq!(rig.server.file_count(), 0);
}
