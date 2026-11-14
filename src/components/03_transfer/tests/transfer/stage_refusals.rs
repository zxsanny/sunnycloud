//! `stage_small` refusals (AZ-3737, IT-03-5): hash, staged cap, quota, free-space floor, in-flight cap and the
//! account state refuse before any byte is written; a failure after the write discards the file.

use contracts::UploadError;
use sc_ids::hash_block;
use uuid::Uuid;

use crate::put_support::content;
use crate::stage_support::stalled;
use crate::start_support::{MIB, Rig, transfer_config};

#[tokio::test]
async fn stage_small_a_wrong_hash_or_bytes_without_a_hash_write_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(1, 1000);
    let mut flipped = data.clone();
    flipped[500] ^= 1;

    // Act
    let (wrong, _) = rig
        .stage_raw(Uuid::new_v4(), Some(hash_block(&data)), &flipped, 1000)
        .await;
    let (missing, _) = rig.stage_raw(Uuid::new_v4(), None, &data, 1000).await;

    // Assert
    assert!(matches!(wrong, Err(UploadError::HashMismatch)), "{wrong:?}");
    assert!(
        matches!(missing, Err(UploadError::HashMismatch)),
        "{missing:?}"
    );
    assert_eq!(rig.writer.completes(), 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_a_thousand_staged_rows_refuse_the_next_before_its_body() {
    // Arrange
    let rig = Rig::new().await;
    rig.fill_staged(1000).await;
    let data = content(2, 1000);

    // Act
    let (result, taken) = rig
        .stage_raw(Uuid::new_v4(), Some(hash_block(&data)), &data, 1000)
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::StagedCapReached)),
        "{result:?}"
    );
    assert_eq!(taken, 0);
    assert_eq!(rig.writer.completes(), 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 1000);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_an_exhausted_quota_refuses_before_the_body() {
    // Arrange
    let rig = Rig::new().await;
    rig.exec(&format!(
        "UPDATE quota SET limit_bytes = {}, used_bytes = {} WHERE account_id = '{}'",
        MIB, MIB, rig.account
    ))
    .await;
    let data = content(3, 1);

    // Act
    let (result, taken) = rig
        .stage_raw(Uuid::new_v4(), Some(hash_block(&data)), &data, 1)
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::QuotaExceeded)),
        "{result:?}"
    );
    assert_eq!(taken, 0);
    assert_eq!(rig.writer.completes(), 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_below_the_free_space_floor_no_file_is_created() {
    // Arrange: the real 04 writer on a volume whose floor is its whole size.
    let rig = Rig::below_floor().await;
    let data = content(4, 1000);

    // Act
    let result = rig.stage(Uuid::new_v4(), &data).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::FreeSpaceFloor)),
        "{result:?}"
    );
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_a_failed_reservation_after_the_write_discards_the_file() {
    // Arrange
    let rig = Rig::new().await;
    rig.ledger.fail_reserve(true);
    let data = content(5, 1000);

    // Act
    let result = rig.stage(Uuid::new_v4(), &data).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::Unexpected(_))),
        "{result:?}"
    );
    assert_eq!(
        rig.writer.completes(),
        1,
        "the file was written before the reservation"
    );
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_the_account_in_flight_cap_is_overloaded_without_reading_the_body() {
    // Arrange: one block in flight per account; a stage holds it while its body is still arriving.
    let mut config = transfer_config();
    config.max_inflight_blocks_per_account = 1;
    let rig = Rig::with_config(config).await;
    let (body, polled) = stalled();
    let service = rig.service.clone();
    let account = rig.account;
    let held = tokio::spawn(async move {
        service
            .stage_small(account, Uuid::new_v4(), None, body, 10)
            .await
    });
    polled.notified().await;
    let data = content(6, 100);

    // Act
    let (refused, taken) = rig
        .stage_raw(Uuid::new_v4(), Some(hash_block(&data)), &data, 100)
        .await;
    held.abort();
    let aborted = held.await;
    let after = rig.stage(Uuid::new_v4(), &data).await;

    // Assert
    assert!(
        matches!(refused, Err(UploadError::Overloaded)),
        "{refused:?}"
    );
    assert_eq!(taken, 0);
    assert!(aborted.is_err_and(|error| error.is_cancelled()));
    assert_eq!(after.expect("staged once the slot is free").size, 100);
    assert_eq!(rig.sessions().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_an_account_that_is_not_active_is_refused() {
    // Arrange
    let rig = Rig::new().await;
    rig.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        rig.account
    ))
    .await;
    let data = content(7, 100);

    // Act
    let (result, taken) = rig
        .stage_raw(Uuid::new_v4(), Some(hash_block(&data)), &data, 100)
        .await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::AccountNotActive)),
        "{result:?}"
    );
    assert_eq!(taken, 0);
    assert_eq!(rig.writer.completes(), 0);
    assert_eq!(rig.files(), 0);
    assert_eq!(rig.sessions().await, 0);
    rig.finish().await;
}
