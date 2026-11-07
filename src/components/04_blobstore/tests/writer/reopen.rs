//! AC-6: `open_for_write` after a dropped handle. AC-7: block bounds.

use std::fs;

use bytes::Bytes;
use contracts::{BlobError, BlobWriter};
use uuid::Uuid;

use crate::support::{MIB, Rig, block_bytes, blocks_of, created, decrypt};

async fn write(rig: &Rig, handle: &contracts::BlobHandle, source: &[u8], index: u32) {
    let token = rig
        .writer
        .write_block(handle, index, block_bytes(source, index))
        .await
        .expect("block is written");
    rig.writer.await_durable(token).await.expect("durable");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_half_written_blob_continues_through_open_for_write_with_the_row_key_material() {
    // Arrange
    let rig = Rig::new().await;
    let size = 8 * MIB;
    let (handle, source) = created(&rig, size, false).await;
    write(&rig, &handle, &source, 3).await;
    write(&rig, &handle, &source, 0).await;
    let plan = handle.plan.clone();
    drop(handle);

    // Act
    let reopened = rig
        .writer
        .open_for_write(
            plan.blob_id,
            &plan.wrapped_key,
            plan.master_key_id,
            size as i64,
        )
        .await
        .expect("open_for_write");
    write(&rig, &reopened, &source, 2).await;
    write(&rig, &reopened, &source, 1).await;

    // Assert
    assert_eq!(reopened.plan, plan);
    assert_eq!(reopened.plaintext_size, size as i64);
    assert_eq!(decrypt(&rig, &plan, size), *source);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_master_key_id_or_a_foreign_wrapped_key_is_key_unavailable() {
    // Arrange
    let rig = Rig::new().await;
    let (handle, _) = created(&rig, MIB, false).await;
    let other = rig.writer.plan(Uuid::new_v4(), 1).expect("plan");
    let plan = handle.plan;

    // Act
    let unknown_id = rig
        .writer
        .open_for_write(plan.blob_id, &plan.wrapped_key, 99, MIB as i64)
        .await;
    let foreign_key = rig
        .writer
        .open_for_write(
            plan.blob_id,
            &other.wrapped_key,
            plan.master_key_id,
            MIB as i64,
        )
        .await;

    // Assert
    assert!(
        matches!(unknown_id, Err(BlobError::KeyUnavailable)),
        "{unknown_id:?}"
    );
    assert!(
        matches!(foreign_key, Err(BlobError::KeyUnavailable)),
        "{foreign_key:?}"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_file_is_blob_missing_and_a_file_without_a_header_is_corrupt() {
    // Arrange
    let rig = Rig::new().await;
    let (deleted, _) = created(&rig, MIB, false).await;
    rig.writer
        .discard(deleted.plan.blob_id)
        .await
        .expect("discard");
    let (stub, _) = created(&rig, MIB, false).await;
    fs::write(rig.path(stub.plan.blob_id), b"short").expect("overwrite");

    // Act
    let gone = rig
        .writer
        .open_for_write(
            deleted.plan.blob_id,
            &deleted.plan.wrapped_key,
            deleted.plan.master_key_id,
            MIB as i64,
        )
        .await;
    let bad = rig
        .writer
        .open_for_write(
            stub.plan.blob_id,
            &stub.plan.wrapped_key,
            stub.plan.master_key_id,
            MIB as i64,
        )
        .await;

    // Assert
    assert!(matches!(gone, Err(BlobError::BlobMissing)), "{gone:?}");
    assert!(matches!(bad, Err(BlobError::Corrupt)), "{bad:?}");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_index_beyond_the_declared_size_is_out_of_range_and_writes_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let size = 5 * MIB;
    let (handle, source) = created(&rig, size, false).await;
    let before = fs::read(rig.path(handle.plan.blob_id)).expect("file");

    // Act
    let first_past = rig
        .writer
        .write_block(&handle, 3, block_bytes(&source, 2))
        .await;
    let far_past = rig
        .writer
        .write_block(&handle, u32::MAX, Bytes::from_static(b"x"))
        .await;

    // Assert
    assert_eq!(blocks_of(size), 3);
    assert!(
        matches!(first_past, Err(BlobError::OutOfRange)),
        "{first_past:?}"
    );
    assert!(
        matches!(far_past, Err(BlobError::OutOfRange)),
        "{far_past:?}"
    );
    assert_eq!(
        fs::read(rig.path(handle.plan.blob_id)).expect("file"),
        before
    );
    assert_eq!(rig.hook.flush_count(), 0);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_last_block_is_in_range_with_its_own_length_and_a_wrong_length_is_size_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    let size = 5 * MIB + 7;
    let (handle, source) = created(&rig, size, false).await;

    // Act
    let last = rig
        .writer
        .write_block(&handle, 2, block_bytes(&source, 2))
        .await;
    let full_length_last = rig
        .writer
        .write_block(&handle, 2, block_bytes(&source, 1))
        .await;
    let short_first = rig
        .writer
        .write_block(&handle, 0, Bytes::from_static(b"short"))
        .await;

    // Assert
    let token = last.expect("the last block of 1 MiB + 7 bytes");
    rig.writer.await_durable(token).await.expect("durable");
    assert!(
        matches!(full_length_last, Err(BlobError::SizeMismatch)),
        "{full_length_last:?}"
    );
    assert!(
        matches!(short_first, Err(BlobError::SizeMismatch)),
        "{short_first:?}"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_empty_blob_has_exactly_one_empty_block() {
    // Arrange
    let rig = Rig::new().await;
    let (handle, source) = created(&rig, 0, true).await;

    // Act
    let only = rig.writer.write_block(&handle, 0, Bytes::new()).await;
    let next = rig.writer.write_block(&handle, 1, Bytes::new()).await;

    // Assert
    rig.writer
        .await_durable(only.expect("block 0"))
        .await
        .expect("durable");
    assert!(matches!(next, Err(BlobError::OutOfRange)), "{next:?}");
    assert_eq!(decrypt(&rig, &handle.plan, 0), *source);
    rig.finish().await;
}
