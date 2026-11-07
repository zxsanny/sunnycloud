//! AC-4 and AC-5: `write_complete` writes and syncs the whole object and no row; `discard`.
//!
//! `NOT-RUN(C-priv)`: a real full loop volume (`ENOSPC` from the operating system in the middle of a write).
//! The `NoSpace` and `FreeSpaceFloor` refusals before the first byte are tested against the scratch volume.

use contracts::{BlobError, BlobWriter};
use uuid::Uuid;

use crate::support::{
    MIB, Rig, Settings, created, data, decrypt, failing_after, stream_of, write_in_order,
};

#[tokio::test(flavor = "multi_thread")]
async fn good_streams_return_written_blob_with_the_file_synced_and_no_row() {
    // Arrange
    let rig = Rig::new().await;
    for size in [0, 1, 3 * MIB] {
        let source = data(size as u64 + 3, size);
        let flushes = rig.hook.flush_count();

        // Act
        let written = rig
            .writer
            .write_complete(Uuid::new_v4(), size as i64, stream_of(&source, 100_000))
            .await
            .expect("write_complete");

        // Assert
        assert_eq!(written.size, size as i64);
        assert_eq!(
            decrypt(&rig, &written.plan, size),
            source,
            "round trip {size}"
        );
        assert_eq!(rig.rows_for(written.plan.blob_id).await, 0);
        assert_eq!(
            rig.hook.flush_count(),
            flushes + 1,
            "one sync group for {size}"
        );
        let synced: Vec<_> = rig
            .hook
            .records()
            .into_iter()
            .filter(|record| record.blob_id == written.plan.blob_id)
            .collect();
        assert_eq!(synced.len(), 1, "the file was synced once");
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_one_byte_short_or_long_is_size_mismatch_and_leaves_no_file() {
    // Arrange
    let rig = Rig::new().await;
    let size = 3 * MIB;
    let source = data(1, size);

    // Act
    let short = rig
        .writer
        .write_complete(
            Uuid::new_v4(),
            size as i64,
            stream_of(&source[..size - 1], 70_000),
        )
        .await;
    let mut longer = source.clone();
    longer.push(0);
    let long = rig
        .writer
        .write_complete(Uuid::new_v4(), size as i64, stream_of(&longer, 70_000))
        .await;

    // Assert
    assert!(matches!(short, Err(BlobError::SizeMismatch)), "{short:?}");
    assert!(matches!(long, Err(BlobError::SizeMismatch)), "{long:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_errors_midway_is_io_and_leaves_no_file() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(2, 3 * MIB);

    // Act
    let result = rig
        .writer
        .write_complete(
            Uuid::new_v4(),
            source.len() as i64,
            failing_after(&source[..2 * MIB + 5]),
        )
        .await;

    // Assert
    assert!(matches!(result, Err(BlobError::Io(_))), "{result:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_size_the_volume_cannot_hold_is_no_space_and_leaves_no_file() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let result = rig
        .writer
        .write_complete(Uuid::new_v4(), 1_i64 << 47, stream_of(&[1, 2, 3], 3))
        .await;

    // Assert
    assert!(matches!(result, Err(BlobError::NoSpace)), "{result:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_below_the_floor_is_free_space_floor_and_leaves_no_file() {
    // Arrange
    let rig = Rig::with(Settings {
        reserve: u64::MAX / 2,
        ..Settings::default()
    })
    .await;

    // Act
    let result = rig
        .writer
        .write_complete(Uuid::new_v4(), 3, stream_of(&[1, 2, 3], 3))
        .await;

    // Assert
    assert!(
        matches!(result, Err(BlobError::FreeSpaceFloor)),
        "{result:?}"
    );
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_sync_is_io_and_removes_the_file() {
    // Arrange
    let rig = Rig::new().await;
    rig.hook.fail_next_flushes(1);
    let source = data(4, MIB);

    // Act
    let result = rig
        .writer
        .write_complete(
            Uuid::new_v4(),
            source.len() as i64,
            stream_of(&source, 50_000),
        )
        .await;

    // Assert
    assert!(matches!(result, Err(BlobError::Io(_))), "{result:?}");
    assert!(rig.blob_files().is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_negative_declared_size_is_out_of_range() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let result = rig
        .writer
        .write_complete(Uuid::new_v4(), -1, stream_of(&[], 1))
        .await;

    // Assert
    assert!(matches!(result, Err(BlobError::OutOfRange)), "{result:?}");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn discard_unlinks_an_existing_file_and_a_missing_one_is_ok() {
    // Arrange
    let rig = Rig::new().await;
    let (handle, source) = created(&rig, MIB, false).await;
    write_in_order(&rig.writer, &handle, &source).await;
    let blob_id = handle.plan.blob_id;
    assert!(rig.path(blob_id).exists());

    // Act
    let existing = rig.writer.discard(blob_id).await;
    let again = rig.writer.discard(blob_id).await;
    let unknown = rig.writer.discard(Uuid::new_v4()).await;

    // Assert
    existing.expect("discard of an existing file");
    again.expect("discard of a missing file");
    unknown.expect("discard of an unknown id");
    assert!(!rig.path(blob_id).exists());
    rig.finish().await;
}
