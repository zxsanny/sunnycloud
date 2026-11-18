//! AC-1: one run at a time, and no lease after a process dies.

use std::time::Duration;

use mobile_backup::{BackupOutcome, SkipReason, Trigger};

use crate::support::{Rig, ctx};

fn rig_with_100_assets() -> Rig {
    let rig = Rig::new(8, 50);
    for id in 1..=100 {
        rig.source.add(id, 1024);
    }
    rig
}

#[tokio::test]
async fn a_second_run_while_one_runs_is_skipped_with_no_sync_call_and_later_runs_proceed() {
    // Arrange
    let rig = rig_with_100_assets();
    let gate = rig.api.close_gate();
    let first = tokio::spawn({
        let (backup, source) = (rig.backup.clone(), rig.source.clone());
        async move { backup.run_backup(ctx(Trigger::MediaChange), source).await }
    });
    gate.entered.notified().await;
    let calls_before = rig.api.record().calls;

    // Act
    let second = rig.try_run(Trigger::AppOpen).await.expect("second run");
    let calls_after = rig.api.record().calls;
    rig.api.open_gate(&gate);
    let first = first.await.expect("first task").expect("first run");
    let third = rig.run(Trigger::CatchUp).await;

    // Assert
    assert_eq!(second, BackupOutcome::Skipped(SkipReason::AlreadyRunning));
    assert_eq!(
        calls_after, calls_before,
        "the skipped run made a sync call"
    );
    assert_eq!(first, BackupOutcome::Completed);
    assert_eq!(third, BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 100);
}

#[tokio::test]
async fn a_run_after_a_process_restart_during_a_run_starts_without_waiting_for_a_lease() {
    // Arrange
    let mut rig = rig_with_100_assets();
    let gate = rig.api.close_gate();
    let dying = tokio::spawn({
        let (backup, source) = (rig.backup.clone(), rig.source.clone());
        async move { backup.run_backup(ctx(Trigger::MediaChange), source).await }
    });
    gate.entered.notified().await;
    dying.abort();
    assert!(dying.await.expect_err("the run was cut").is_cancelled());

    // Act
    rig.restart();
    let outcome = tokio::time::timeout(Duration::from_secs(30), rig.run(Trigger::AppOpen))
        .await
        .expect("the run did not wait");

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 100);
    assert_eq!(rig.db.count("uploaded"), 100);
}
