//! AZ-3997 AC-2 and AC-3: a camera folder the server no longer has is recreated once and the asset sent again;
//! a second refusal ends the run without a third try.

use mobile_backup::{BackupOutcome, RunError, Trigger};
use sync_core::SyncError;
use uuid::Uuid;

use crate::support::server::ROOT;
use crate::support::{LABEL, Rig};

const LARGE: usize = 2 * 1024 * 1024 + 1;
const SMALL: usize = 1024;

fn name(id: i64) -> String {
    format!("IMG_{id}.jpg")
}

fn device_folder(rig: &Rig) -> Option<Uuid> {
    let state = rig.server.state();
    let camera = state.folder_named(ROOT, "Camera Uploads")?;
    state.folder_named(camera, LABEL)
}

fn attempts_of(rig: &Rig, asset_id: i64) -> usize {
    rig.api
        .record()
        .attempts
        .iter()
        .filter(|(sent, _)| *sent == name(asset_id))
        .count()
}

/// One uploaded asset, then the user deletes the folder on the web and two more assets of the same size arrive.
async fn rig_with_deleted_folder(size: usize, not_found: bool) -> (Rig, Uuid) {
    let rig = Rig::new(8, 50);
    rig.source.add(1, size);
    rig.run(Trigger::FirstSync).await;
    let old = device_folder(&rig).expect("the device folder");
    rig.server.delete_folder(old);
    rig.server.state().missing_is_not_found = not_found;
    rig.source.add(2, size);
    rig.source.add(3, size);
    (rig, old)
}

/// `second_attempts`: how often the third asset is sent. A block upload goes alone, straight into the new folder
/// (1); small files of one batch are all refused with the folder and sent together again (2).
async fn assert_folder_recreated_and_asset_retried_once(
    size: usize,
    not_found: bool,
    second_attempts: usize,
) {
    // Arrange
    let (rig, old) = rig_with_deleted_folder(size, not_found).await;
    let creates_before = rig.api.record().creates;

    // Act
    let outcome = rig.run(Trigger::MediaChange).await;

    // Assert
    let new = device_folder(&rig).expect("the folder was made again");
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_ne!(new, old);
    assert_eq!(rig.db.cursor_text("camera_folder"), Some(new.to_string()));
    assert_eq!(rig.api.record().creates - creates_before, 1);
    assert_eq!(
        (attempts_of(&rig, 2), attempts_of(&rig, 3)),
        (2, second_attempts),
        "the refused asset is sent twice, never a third time"
    );
    let state = rig.server.state();
    let in_new = state
        .files
        .values()
        .filter(|file| file.parent == new)
        .count();
    assert_eq!(in_new, 2);
    assert_eq!(rig.db.count("uploaded"), 3);
}

#[tokio::test]
async fn a_deleted_folder_is_recreated_and_the_block_upload_sent_again_on_parent_gone() {
    assert_folder_recreated_and_asset_retried_once(LARGE, false, 1).await;
}

#[tokio::test]
async fn a_deleted_folder_is_recreated_and_the_block_upload_sent_again_on_parent_not_found() {
    assert_folder_recreated_and_asset_retried_once(LARGE, true, 1).await;
}

#[tokio::test]
async fn a_deleted_folder_is_recreated_and_the_refused_batch_entries_sent_again_on_parent_deleted()
{
    assert_folder_recreated_and_asset_retried_once(SMALL, false, 2).await;
}

#[tokio::test]
async fn a_deleted_folder_is_recreated_and_the_refused_batch_entries_sent_again_on_parent_not_found()
 {
    assert_folder_recreated_and_asset_retried_once(SMALL, true, 2).await;
}

#[tokio::test]
async fn a_block_upload_refused_again_after_the_folder_was_recreated_ends_the_run_with_the_error_noted()
 {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, LARGE);
    rig.source.add(2, LARGE);
    rig.api.fail_upload(&name(1), SyncError::ParentGone);

    // Act
    let outcome = rig.try_run(Trigger::FirstSync).await;

    // Assert
    assert!(
        matches!(outcome, Err(RunError::Sync(SyncError::ParentGone))),
        "{outcome:?}"
    );
    assert_eq!(attempts_of(&rig, 1), 2, "one try and one retry");
    assert_eq!(
        attempts_of(&rig, 2),
        0,
        "the run ended before the next asset"
    );
    let status = rig.core.status().expect("status");
    assert_eq!(status.pending, 2);
    let error = status.last_error.expect("the failure is kept");
    assert_eq!(error.correlation_id.len(), 36, "a fresh correlation id");
}

#[tokio::test]
async fn a_batch_entry_refused_again_after_the_folder_was_recreated_ends_the_run_with_the_error_noted()
 {
    // Arrange
    let rig = Rig::new(8, 50);
    rig.source.add(1, SMALL);
    rig.source.add(2, SMALL);
    rig.server
        .state()
        .refused
        .insert(name(1), "parent_deleted".to_owned());

    // Act
    let outcome = rig.try_run(Trigger::FirstSync).await;

    // Assert
    assert!(
        matches!(outcome, Err(RunError::Sync(SyncError::ParentGone))),
        "{outcome:?}"
    );
    assert_eq!(attempts_of(&rig, 1), 2, "one try and one retry");
    assert_eq!(
        attempts_of(&rig, 2),
        1,
        "the accepted entry is not sent again"
    );
    let status = rig.core.status().expect("status");
    assert_eq!((status.pending, status.uploaded), (1, 1));
    assert!(status.last_error.is_some());
}
