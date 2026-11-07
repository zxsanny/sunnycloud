//! AC-1: planning is pure.

use contracts::{BlobError, BlobWriter};
use uuid::Uuid;

use crate::support::{MIB, Rig};

#[tokio::test]
async fn plan_returns_id_key_and_master_key_id_and_creates_no_file_and_no_row() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let plan = rig
        .writer
        .plan(Uuid::new_v4(), 5 * MIB as i64)
        .expect("plan");

    // Assert
    assert_eq!(plan.blob_id.get_version_num(), 4);
    assert_eq!(plan.master_key_id, 1);
    assert_eq!(plan.wrapped_key.len(), 24 + 32 + 16);
    assert!(!rig.path(plan.blob_id).exists());
    assert!(!rig.volume.join("blobs").exists(), "no folder was created");
    assert_eq!(rig.rows_for(plan.blob_id).await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn two_plans_differ_in_id_and_wrapped_key() {
    // Arrange
    let rig = Rig::new().await;
    let account = Uuid::new_v4();

    // Act
    let first = rig.writer.plan(account, 1).expect("plan");
    let second = rig.writer.plan(account, 1).expect("plan");

    // Assert
    assert_ne!(first.blob_id, second.blob_id);
    assert_ne!(first.wrapped_key, second.wrapped_key);
    rig.finish().await;
}

#[tokio::test]
async fn a_negative_size_and_a_size_past_the_chunk_counter_are_out_of_range() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let negative = rig.writer.plan(Uuid::new_v4(), -1);
    let huge = rig.writer.plan(Uuid::new_v4(), i64::MAX);

    // Assert
    assert!(matches!(negative, Err(BlobError::OutOfRange)));
    assert!(matches!(huge, Err(BlobError::OutOfRange)));
    rig.finish().await;
}

#[tokio::test]
async fn plan_without_a_loaded_keyring_is_key_unavailable() {
    // Arrange
    let rig = Rig::new().await;
    let unloaded = rig.unloaded_writer();

    // Act
    let plan = unloaded.plan(Uuid::new_v4(), 1);

    // Assert
    assert!(matches!(plan, Err(BlobError::KeyUnavailable)));
    drop(unloaded);
    rig.finish().await;
}
