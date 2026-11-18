//! AC-3: every sync-core error ends in a state of the table of `backup_core.md` § Wire errors to checkpoint
//! states and pauses, and the loop goes on for the errors of single assets.

use std::io::ErrorKind;

use mobile_backup::{
    BackupOutcome, NetworkPolicy, NetworkState, PauseReason, RunContext, RunError, Trigger,
};
use sync_core::SyncError;

use crate::support::{Rig, ctx};

const LARGE: usize = 2 * 1024 * 1024 + 1;
const SMALL: usize = 1024;

fn name(id: i64) -> String {
    format!("IMG_{id}.jpg")
}

/// Hashes, block uploads and batches made so far: what a retried asset would add.
fn sent(rig: &Rig) -> (usize, usize, usize) {
    let record = rig.api.record();
    (record.hashed.len(), record.large.len(), record.groups.len())
}

fn rig_with(sizes: &[usize]) -> Rig {
    let rig = Rig::new(8, 50);
    for (index, size) in sizes.iter().enumerate() {
        rig.source.add(index as i64 + 1, *size);
    }
    rig
}

#[tokio::test]
async fn a_507_on_a_block_upload_pauses_the_run_with_the_asset_pending_and_the_next_run_uploads_it()
{
    // Arrange
    let rig = rig_with(&[LARGE, LARGE]);
    rig.api.fail_upload(&name(1), SyncError::QuotaExceeded);

    // Act
    let paused = rig.run(Trigger::FirstSync).await;
    let status = rig.core.status().expect("status");
    rig.api.clear_faults();
    let resumed = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(paused, BackupOutcome::Paused(PauseReason::QuotaExceeded));
    assert_eq!(status.paused, Some(PauseReason::QuotaExceeded));
    assert_eq!((status.uploaded, status.pending), (0, 2));
    assert_eq!(resumed, BackupOutcome::Completed);
    assert_eq!(rig.core.status().expect("status").paused, None);
    assert_eq!(rig.server.file_count(), 2);
}

#[tokio::test]
async fn a_507_on_a_batch_pauses_the_run_and_the_free_space_floor_reads_the_same() {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL, SMALL]);
    rig.api.fail_upload(&name(2), SyncError::FreeSpaceFloor);

    // Act
    let paused = rig.run(Trigger::FirstSync).await;
    let status = rig.core.status().expect("status");
    rig.api.clear_faults();
    let resumed = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(paused, BackupOutcome::Paused(PauseReason::QuotaExceeded));
    assert_eq!(
        status.last_error.expect("an error text").message,
        "free space floor reached"
    );
    assert_eq!(status.pending, 3);
    assert_eq!(resumed, BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 3);
}

#[tokio::test]
async fn size_too_large_ends_the_asset_skipped_size_on_both_paths_and_it_is_not_retried() {
    // Arrange
    let rig = rig_with(&[SMALL, LARGE, SMALL, LARGE]);
    rig.api.fail_upload(&name(1), SyncError::SizeTooLarge);
    rig.api.fail_upload(&name(2), SyncError::SizeTooLarge);

    // Act
    let first = rig.run(Trigger::FirstSync).await;
    let sent_first = sent(&rig);
    let second = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(first, BackupOutcome::Completed);
    assert_eq!(second, BackupOutcome::Completed);
    assert_eq!(rig.db.count("skipped_size"), 2);
    assert_eq!(rig.db.count("uploaded"), 2);
    let status = rig.core.status().expect("status");
    assert_eq!((status.skipped.size, status.pending), (2, 0));
    assert_eq!(
        sent(&rig),
        sent_first,
        "the second run hashed or uploaded an ended asset"
    );
}

#[tokio::test]
async fn a_rejected_name_ends_the_asset_skipped_name_whether_it_is_an_upload_error_or_a_batch_entry_error()
 {
    // Arrange
    let rig = rig_with(&[LARGE, SMALL, SMALL]);
    rig.api.fail_upload(&name(1), SyncError::InvalidName);
    rig.server
        .state()
        .refused
        .insert(name(2), "invalid_name".to_owned());

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(rig.db.count("skipped_name"), 2);
    assert_eq!(rig.db.count("uploaded"), 1);
    assert_eq!(rig.core.status().expect("status").skipped.name, 2);
}

#[tokio::test]
async fn a_file_that_vanishes_while_it_is_hashed_ends_skipped_gone_and_the_next_asset_uploads() {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL]);
    rig.api.fail_hash(
        1,
        SyncError::Io {
            kind: ErrorKind::NotFound,
        },
    );

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(rig.db.count("skipped_gone"), 1);
    assert_eq!(rig.db.count("uploaded"), 1);
    assert_eq!(rig.core.status().expect("status").last_error, None);
}

#[tokio::test]
async fn an_unreadable_file_and_a_changed_file_end_failed_with_an_error_text_and_the_loop_goes_on()
{
    // Arrange
    let rig = rig_with(&[SMALL, SMALL, SMALL, SMALL]);
    rig.api.fail_hash(
        1,
        SyncError::Io {
            kind: ErrorKind::Other,
        },
    );
    rig.api.fail_upload(&name(2), SyncError::SourceChanged);

    // Act
    let first = rig.run(Trigger::FirstSync).await;
    let sent_first = sent(&rig);
    let second = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(first, BackupOutcome::Completed);
    assert_eq!(second, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    assert_eq!(
        (status.skipped.failed, status.uploaded, status.pending),
        (2, 2, 0)
    );
    assert_eq!(rig.db.count("failed"), 2);
    assert_eq!(sent(&rig), sent_first, "a failed asset was retried");
}

#[tokio::test]
async fn an_unexpected_server_answer_fails_the_asset_with_its_request_id_as_the_correlation_id() {
    // Arrange
    let rig = rig_with(&[LARGE, LARGE]);
    rig.api.fail_upload(
        &name(1),
        SyncError::Unexpected {
            code: "internal".to_owned(),
            request_id: Some("req-77".to_owned()),
        },
    );

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    let error = status.last_error.expect("the error is kept");
    assert_eq!(error.correlation_id, "req-77");
    assert!(error.message.contains("internal"), "{}", error.message);
    assert_eq!((status.skipped.failed, status.uploaded), (1, 1));
}

#[tokio::test]
async fn an_error_without_a_request_id_gets_a_fresh_correlation_id_per_failure() {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL]);
    for id in 1..=2 {
        rig.api.fail_hash(id, SyncError::SourceChanged);
    }

    // Act
    rig.run(Trigger::FirstSync).await;

    // Assert
    let error = rig
        .core
        .status()
        .expect("status")
        .last_error
        .expect("an error");
    assert_eq!(error.message, "source changed");
    assert_eq!(error.correlation_id.len(), 36, "a UUID text");
}

#[tokio::test]
async fn a_file_a_batch_leaves_out_ends_failed_and_is_not_left_pending() {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL]);
    rig.api.fail_upload(
        &name(1),
        SyncError::Io {
            kind: ErrorKind::TimedOut,
        },
    );

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    assert_eq!(
        (status.skipped.failed, status.uploaded, status.pending),
        (1, 1, 0)
    );
    assert_eq!(
        status.last_error.expect("an error").message,
        "io error (TimedOut)"
    );
}

#[tokio::test]
async fn a_batch_entry_refused_with_an_unknown_code_fails_its_asset_without_repeating_the_server_text()
 {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL]);
    rig.server
        .state()
        .refused
        .insert(name(1), "a_code_from_the_future".to_owned());

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    assert_eq!((status.skipped.failed, status.uploaded), (1, 1));
    assert_eq!(
        status.last_error.expect("an error").message,
        "batch entry refused (unknown)"
    );
}

#[tokio::test]
async fn an_entry_whose_staged_row_is_gone_stays_pending_and_the_next_run_uploads_it() {
    // Arrange
    let rig = rig_with(&[SMALL, SMALL]);
    rig.server
        .state()
        .refused
        .insert(name(1), "upload_session_gone".to_owned());

    // Act
    let first = rig.run(Trigger::FirstSync).await;
    let pending = rig.core.status().expect("status").pending;
    rig.server.state().refused.clear();
    let second = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(first, BackupOutcome::Completed);
    assert_eq!(pending, 1);
    assert_eq!(second, BackupOutcome::Completed);
    assert_eq!(rig.db.count("uploaded"), 2);
}

#[tokio::test]
async fn an_inactive_account_pauses_the_run_with_the_assets_pending() {
    // Arrange
    let rig = rig_with(&[LARGE, SMALL]);
    rig.api.fail_upload(&name(1), SyncError::AccountNotActive);

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(
        outcome,
        BackupOutcome::Paused(PauseReason::AccountNotActive)
    );
    let status = rig.core.status().expect("status");
    assert_eq!(status.paused, Some(PauseReason::AccountNotActive));
    assert_eq!((status.uploaded, status.pending), (0, 2));
}

#[tokio::test]
async fn a_426_pauses_the_run_and_keeps_the_server_text_in_last_error() {
    // Arrange
    let rig = rig_with(&[SMALL]);
    rig.api.fail_upload(
        &name(1),
        SyncError::UpgradeRequired {
            message: "Please update Sunnycloud".to_owned(),
            min_version: None,
            update_url: None,
        },
    );

    // Act
    let outcome = rig.run(Trigger::FirstSync).await;

    // Assert
    assert_eq!(outcome, BackupOutcome::Paused(PauseReason::UpgradeRequired));
    let status = rig.core.status().expect("status");
    assert_eq!(status.paused, Some(PauseReason::UpgradeRequired));
    assert_eq!(
        status.last_error.expect("the text").message,
        "Please update Sunnycloud"
    );
}

#[tokio::test]
async fn a_rejected_token_ends_the_run_with_the_error_and_leaves_the_asset_pending() {
    // Arrange
    let rig = rig_with(&[LARGE, SMALL]);
    rig.api.fail_upload(&name(1), SyncError::Unauthorized);

    // Act
    let result = rig.try_run(Trigger::FirstSync).await;

    // Assert
    assert!(
        matches!(result, Err(RunError::Sync(SyncError::Unauthorized))),
        "{result:?}"
    );
    let status = rig.core.status().expect("status");
    assert_eq!((status.paused, status.pending), (None, 2));
}

fn with_network(wifi_only: bool, network: NetworkState) -> RunContext {
    RunContext {
        network_policy: NetworkPolicy { wifi_only, network },
        ..ctx(Trigger::AppOpen)
    }
}

#[tokio::test]
async fn a_metered_network_with_wifi_only_on_pauses_before_any_call_and_an_offline_one_reads_no_network()
 {
    // Arrange
    let rig = rig_with(&[SMALL]);

    // Act
    let metered = rig
        .try_run_with(with_network(true, NetworkState::Metered))
        .await
        .expect("an outcome");
    let status = rig.core.status().expect("status");
    let offline = rig
        .try_run_with(with_network(false, NetworkState::Offline))
        .await
        .expect("an outcome");

    // Assert
    assert_eq!(metered, BackupOutcome::Paused(PauseReason::MeteredNetwork));
    assert_eq!(status.paused, Some(PauseReason::MeteredNetwork));
    assert_eq!(offline, BackupOutcome::Paused(PauseReason::NoNetwork));
    assert_eq!(rig.api.record().calls, 0, "a call was made while paused");
    assert_eq!(rig.db.count("new"), 0, "nothing was even listed");
}

#[tokio::test]
async fn a_metered_network_with_wifi_only_off_runs_and_clears_the_pause_of_the_run_before() {
    // Arrange
    let rig = rig_with(&[SMALL]);
    rig.try_run_with(with_network(true, NetworkState::Metered))
        .await
        .expect("an outcome");

    // Act
    let outcome = rig
        .try_run_with(with_network(false, NetworkState::Metered))
        .await
        .expect("an outcome");

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(rig.core.status().expect("status").paused, None);
    assert_eq!(rig.server.file_count(), 1);
}

#[tokio::test]
async fn denied_access_pauses_with_permission_denied_and_a_grant_lets_the_next_run_proceed() {
    // Arrange
    let rig = rig_with(&[SMALL]);
    rig.source.set_denied(true);

    // Act
    let denied = rig.run(Trigger::AppOpen).await;
    let status = rig.core.status().expect("status");
    let calls = rig.api.record().calls;
    rig.source.set_denied(false);
    let granted = rig.run(Trigger::AppOpen).await;

    // Assert
    assert_eq!(denied, BackupOutcome::Paused(PauseReason::PermissionDenied));
    assert_eq!(status.paused, Some(PauseReason::PermissionDenied));
    assert_eq!(status.access_level, mobile_backup::AccessLevel::Denied);
    assert_eq!(calls, 0);
    assert_eq!(granted, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    assert_eq!(status.access_level, mobile_backup::AccessLevel::Full);
    assert_eq!(status.uploaded, 1);
}
