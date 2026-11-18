//! AC-5: `cancel` ends the run between blocks and the operations in flight, and nothing is sent afterwards.

use std::time::Duration;

use mobile_backup::{BackupOutcome, Trigger};

use crate::support::{Rig, ctx};

const LARGE: usize = 2 * 1024 * 1024 + 1;

async fn cancelled_run_with_uploads_in_flight(rig: &Rig) -> (BackupOutcome, usize) {
    let gate = rig.api.hold_uploads();
    let run = tokio::spawn({
        let (backup, source) = (rig.backup.clone(), rig.source.clone());
        async move { backup.run_backup(ctx(Trigger::FirstSync), source).await }
    });
    gate.entered.notified().await;
    let calls_before = rig.api.record().calls;

    rig.core.cancel();

    let outcome = tokio::time::timeout(Duration::from_secs(1), run)
        .await
        .expect("the run ended within 1 s of the cancel")
        .expect("the run task")
        .expect("an outcome");
    (outcome, calls_before)
}

#[tokio::test]
async fn cancel_with_three_staged_uploads_in_flight_ends_the_run_partial_and_sends_nothing_more() {
    // Arrange
    let rig = Rig::new(8, 50);
    for id in 1..=3 {
        rig.source.add(id, 1024);
    }

    // Act
    let (outcome, calls_before) = cancelled_run_with_uploads_in_flight(&rig).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Partial { remaining: 3 });
    assert_eq!(
        rig.api.record().calls,
        calls_before,
        "a call after the cancel"
    );
    assert_eq!(rig.server.file_count(), 0);
    assert_eq!(rig.core.status().expect("status").pending, 3);
}

#[tokio::test]
async fn cancel_during_a_block_upload_ends_the_run_partial_with_the_asset_pending() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, LARGE);
    rig.source.add(2, 1024);

    // Act
    let (outcome, calls_before) = cancelled_run_with_uploads_in_flight(&rig).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Partial { remaining: 2 });
    assert_eq!(rig.api.record().calls, calls_before);
    assert_eq!(rig.server.file_count(), 0);
}

#[tokio::test]
async fn a_cancel_with_no_run_in_progress_does_nothing_and_the_next_run_completes() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 1024);

    // Act
    rig.core.cancel();
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 1);
}
