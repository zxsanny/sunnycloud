//! AC-4: the counts of `status` add up at every read, through vanished assets and a full quota.

use std::sync::{Arc, Mutex};

use mobile_backup::{BackupOutcome, BackupStatus, PauseReason, Trigger};

use crate::support::Rig;

fn adds_up(status: &BackupStatus) -> bool {
    status.visible
        == status.uploaded
            + status.pending
            + status.skipped.gone
            + status.skipped.name
            + status.skipped.size
            + status.skipped.failed
}

#[tokio::test]
async fn the_counts_add_up_at_every_read_of_two_runs_with_five_vanished_assets_and_a_full_quota() {
    // Arrange
    let rig = Rig::new(8, 1);
    for id in 1..=100 {
        rig.source.add(id, 1024);
    }
    for id in [2, 4, 6, 8, 10] {
        rig.source.vanish_on_open(id);
    }
    rig.api.quota_after(10);
    let reads: Arc<Mutex<Vec<BackupStatus>>> = Arc::default();
    rig.api.on_call({
        let (core, reads) = (Arc::clone(&rig.core), Arc::clone(&reads));
        move |_| {
            reads
                .lock()
                .expect("reads")
                .push(core.status().expect("status"));
        }
    });

    // Act
    let first = rig.run(Trigger::FirstSync).await;
    let after_first = rig.core.status().expect("status");
    rig.api.raise_quota();
    let second = rig.run(Trigger::AppOpen).await;
    let after_second = rig.core.status().expect("status");

    // Assert
    assert_eq!(first, BackupOutcome::Paused(PauseReason::QuotaExceeded));
    assert_eq!(
        (
            after_first.uploaded,
            after_first.skipped.gone,
            after_first.pending
        ),
        (10, 5, 85)
    );
    assert_eq!(second, BackupOutcome::Completed);
    assert_eq!(after_second.uploaded, 95);
    assert_eq!(after_second.skipped.gone, 5);
    assert_eq!(after_second.pending, 0);
    assert_eq!(rig.server.file_count(), 95);
    let reads = reads.lock().expect("reads");
    assert!(reads.len() > 100, "only {} reads", reads.len());
    for read in reads.iter().chain([&after_first, &after_second]) {
        assert!(adds_up(read), "{read:?}");
        assert!(
            read.visible == 0 || read.visible == 100,
            "visible is the 100 assets once listed: {read:?}"
        );
    }
}

#[tokio::test]
async fn the_visible_count_is_the_rows_of_the_checkpoint_not_the_sum_of_its_parts() {
    // Arrange
    let rig = Rig::new(8, 50);
    for id in 1..=7 {
        rig.source.add(id, 1024);
    }
    rig.run(Trigger::FirstSync).await;
    rig.db
        .exec("UPDATE asset SET state = 'failed' WHERE asset_id = 3");
    rig.db
        .exec("UPDATE asset SET state = 'skipped_name' WHERE asset_id = 4");

    // Act
    let status = rig.core.status().expect("status");

    // Assert
    assert_eq!(status.visible, 7);
    assert_eq!(
        (status.uploaded, status.skipped.failed, status.skipped.name),
        (5, 1, 1)
    );
    assert!(adds_up(&status), "{status:?}");
}
