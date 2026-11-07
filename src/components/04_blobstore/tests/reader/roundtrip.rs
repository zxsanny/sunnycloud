//! AC-1: blobs of the chunk-size corpus read back whole. AC-8: a blob continued after a dropped handle.

use contracts::BlobWriter;

use crate::support::{MIB, Rig, blocks_of, consume, data, shuffled};

#[tokio::test(flavor = "multi_thread")]
async fn blobs_of_every_chunk_edge_size_read_back_to_the_source_bytes_and_hash() {
    // Arrange
    let rig = Rig::new().await;
    let sizes = [0, 1, 65_535, 65_536, 65_537, 2 * MIB, 5 * MIB + 7];

    for size in sizes {
        let source = data(size as u64 + 7, size);
        let blob = rig.store(&source).await;

        // Act
        let stream = rig.open(&blob, None).await.expect("open");
        let consumed = consume(stream).await;

        // Assert
        assert!(
            consumed.error.is_none(),
            "size {size}: {:?}",
            consumed.error
        );
        assert_eq!(consumed.bytes.len(), size, "size {size}");
        assert_eq!(
            blake3::hash(&consumed.bytes),
            blake3::hash(&source),
            "size {size}"
        );
        assert!(consumed.bytes == *source, "size {size}: bytes differ");
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blob_continued_after_a_dropped_handle_reads_back_to_the_source() {
    // Arrange
    let rig = Rig::new().await;
    let size = 8 * MIB + 5;
    let source = data(0xA8, size);
    let plan = rig.writer.plan(rig.account, size as i64).expect("plan");
    let handle = rig.writer.create_file(plan, false).await.expect("create");
    let order = shuffled(blocks_of(size), 0xBEEF);
    let (before, after) = order.split_at(2);
    rig.write_blocks(&handle, &source, before).await;
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
    rig.write_blocks(&reopened, &source, after).await;
    let blob = rig.commit(reopened, size as i64).await;
    let consumed = consume(rig.open(&blob, None).await.expect("open")).await;

    // Assert
    assert!(consumed.error.is_none(), "{:?}", consumed.error);
    assert!(consumed.bytes == *source, "bytes differ");
    rig.finish().await;
}
