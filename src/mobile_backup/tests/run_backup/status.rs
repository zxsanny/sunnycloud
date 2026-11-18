//! The contents of `status` beyond the counts: access level, the location flag and the last error.

use mobile_backup::{AccessLevel, BackupStatus, SkippedCounts, Trigger};
use sync_core::SyncError;

use crate::support::Rig;

#[tokio::test]
async fn a_status_before_any_run_is_empty_with_full_access() {
    // Arrange
    let rig = Rig::new(8, 50);

    // Act
    let status = rig.core.status().expect("status");

    // Assert
    assert_eq!(
        status,
        BackupStatus {
            visible: 0,
            uploaded: 0,
            pending: 0,
            paused: None,
            skipped: SkippedCounts {
                gone: 0,
                name: 0,
                size: 0,
                failed: 0
            },
            last_error: None,
            access_level: AccessLevel::Full,
            location_redacted: false,
        }
    );
}

#[tokio::test]
async fn the_location_flag_is_set_by_a_redacted_original_and_cleared_by_the_next_one_that_is_not() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 1024);
    rig.source.set_redacted(true);
    rig.run(Trigger::FirstSync).await;
    let redacted = rig.core.status().expect("status").location_redacted;
    rig.source.set_redacted(false);
    rig.source.edit(1);

    // Act
    rig.run(Trigger::MediaChange).await;

    // Assert
    assert!(redacted);
    assert!(!rig.core.status().expect("status").location_redacted);
}

#[tokio::test]
async fn limited_access_is_shown_and_the_visible_assets_are_backed_up() {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, 1024);
    rig.source.set_limited(true);

    // Act
    rig.run(Trigger::AppOpen).await;

    // Assert
    let status = rig.core.status().expect("status");
    assert_eq!(status.access_level, AccessLevel::Limited);
    assert_eq!(status.uploaded, 1);
}

#[tokio::test]
async fn the_last_error_survives_a_restart_and_a_run_that_starts_clears_it() {
    // Arrange
    let mut rig = Rig::new(8, 50);
    rig.source.add(1, 1024);
    rig.api.fail_hash(1, SyncError::SourceChanged);
    rig.run(Trigger::FirstSync).await;
    rig.restart();

    // Act
    let after_restart = rig.core.status().expect("status").last_error;
    rig.source.add(2, 1024);
    rig.run(Trigger::MediaChange).await;
    let after_next_run = rig.core.status().expect("status").last_error;

    // Assert
    assert_eq!(after_restart.expect("kept").message, "source changed");
    assert_eq!(after_next_run, None);
}
