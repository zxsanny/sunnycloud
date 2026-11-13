//! AC-3: the free-space floor, preallocation and the single directory sync.
//!
//! `NOT-RUN(C-priv)`: the 64 MiB loop volume with an 8 MiB reserve and a filler file (no loop-volume tool or
//! privileged runner here). The refusal logic runs against the real `statvfs` of the scratch volume.

use std::os::unix::fs::MetadataExt;

use contracts::{BlobError, BlobWriter};
use uuid::Uuid;

use crate::support::{MIB, Rig, Settings, created, write_in_order};

#[tokio::test]
async fn a_file_that_would_leave_less_than_the_reserve_is_free_space_floor_and_leaves_no_file() {
    // Arrange
    let rig = Rig::with(Settings {
        reserve: u64::MAX / 2,
        ..Settings::default()
    })
    .await;
    let plan = rig
        .writer
        .plan(Uuid::new_v4(), 60 * MIB as i64)
        .expect("plan");
    let blob_id = plan.blob_id;

    // Act
    let result = rig.writer.create_file(plan, 60 * MIB as i64, true).await;

    // Assert
    assert!(
        matches!(result, Err(BlobError::FreeSpaceFloor)),
        "{result:?}"
    );
    assert!(!rig.path(blob_id).exists());
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test]
async fn a_file_larger_than_the_volume_is_no_space_and_leaves_no_file() {
    // Arrange
    let rig = Rig::new().await;
    let plan = rig.writer.plan(Uuid::new_v4(), 1_i64 << 47).expect("plan");

    // Act
    let result = rig.writer.create_file(plan, 1_i64 << 47, false).await;

    // Assert
    assert!(matches!(result, Err(BlobError::NoSpace)), "{result:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test]
async fn preallocation_reserves_the_whole_ciphertext_length_at_create() {
    // Arrange
    let rig = Rig::new().await;
    let size = 20 * MIB;

    // Act
    let (handle, source) = created(&rig, size, true).await;

    // Assert
    let meta = std::fs::metadata(rig.path(handle.plan.blob_id)).expect("file");
    let whole = 64 + (size / 65_536) as u64 * 16 + size as u64;
    assert_eq!(meta.len(), whole);
    assert!(
        meta.blocks() * 512 >= whole,
        "blocks are allocated, not a hole"
    );
    write_in_order(&rig.writer, &handle, &source).await;
    assert_eq!(
        std::fs::metadata(rig.path(handle.plan.blob_id))
            .expect("file")
            .len(),
        whole
    );
    rig.finish().await;
}

#[tokio::test]
async fn without_preallocation_the_file_holds_only_the_header_until_blocks_arrive() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let (handle, _) = created(&rig, 20 * MIB, false).await;

    // Assert
    let meta = std::fs::metadata(rig.path(handle.plan.blob_id)).expect("file");
    assert_eq!(meta.len(), 64);
    rig.finish().await;
}

#[tokio::test]
async fn the_configuration_switch_off_overrides_a_preallocate_request() {
    // Arrange
    let rig = Rig::with(Settings {
        preallocate: false,
        ..Settings::default()
    })
    .await;

    // Act
    let (handle, _) = created(&rig, 20 * MIB, true).await;

    // Assert
    let meta = std::fs::metadata(rig.path(handle.plan.blob_id)).expect("file");
    assert_eq!(meta.len(), 64);
    rig.finish().await;
}

#[tokio::test]
async fn the_directory_entry_is_synced_exactly_once_per_created_file() {
    // Arrange
    let rig = Rig::new().await;
    assert_eq!(rig.writer.directory_syncs(), 0);

    // Act
    let (first, source) = created(&rig, 3 * MIB, true).await;
    let after_one = rig.writer.directory_syncs();
    let (_second, _) = created(&rig, MIB, false).await;
    write_in_order(&rig.writer, &first, &source).await;

    // Assert
    assert_eq!(after_one, 1);
    assert_eq!(
        rig.writer.directory_syncs(),
        2,
        "block writes sync no directory"
    );
    rig.finish().await;
}

#[tokio::test]
async fn unused_plans_leave_nothing_in_the_writer_and_another_writer_creates_a_kept_plan() {
    // Arrange
    let rig = Rig::new().await;
    for _ in 0..100 {
        rig.writer
            .plan(Uuid::new_v4(), MIB as i64)
            .expect("unused plan");
    }
    let kept = rig.writer.plan(Uuid::new_v4(), MIB as i64).expect("plan");
    let blob_id = kept.blob_id;
    let other = rig.second_writer();

    // Act
    let handle = other
        .create_file(kept, MIB as i64, false)
        .await
        .expect("create_file needs only the caller's plan and size");

    // Assert
    assert_eq!(handle.plan.blob_id, blob_id);
    assert_eq!(handle.plaintext_size, MIB as i64);
    assert_eq!(rig.blob_files(), vec![rig.path(blob_id)]);
    drop(other);
    rig.finish().await;
}

#[tokio::test]
async fn create_file_with_a_size_past_the_chunk_counter_is_out_of_range_and_creates_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let plan = rig.writer.plan(Uuid::new_v4(), 1).expect("plan");

    // Act
    let result = rig.writer.create_file(plan, i64::MAX, false).await;

    // Assert
    assert!(matches!(result, Err(BlobError::OutOfRange)), "{result:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}
