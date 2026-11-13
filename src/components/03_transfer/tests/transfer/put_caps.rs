//! `put_block` AC-4 (IT-03-2, R13): the lane, account and buffer caps refuse before any body byte is read; and
//! AC-5 (IT-03-3, AC-38): the ack waits for the group fdatasync, and a failed sync leaves the block unreceived.
//! Blocks are held in flight by the writer decorator, which parks `await_durable` until it is released.

use contracts::{BlockAck, UploadError};
use sc_ids::BLOCK_SIZE;

use crate::put_support::{Upload, bit, spawn_put};
use crate::start_ports::DurableFault;
use crate::start_support::{Rig, transfer_config};

const TWO_MIB: u64 = BLOCK_SIZE as u64;
const EIGHT_MIB: usize = 4 * BLOCK_SIZE;

/// Sends `index` of `upload` while every lane is held; returns the answer and the body bytes read.
async fn one_more(
    rig: &Rig,
    upload: &Upload,
    index: u32,
) -> (Result<BlockAck, UploadError>, usize) {
    rig.put_raw(
        upload.id,
        index,
        upload.hash(index),
        upload.block(index),
        TWO_MIB,
    )
    .await
}

async fn join_all(handles: Vec<tokio::task::JoinHandle<Result<BlockAck, UploadError>>>) {
    for handle in handles {
        let result = handle.await.expect("lane task");
        assert!(matches!(result, Ok(BlockAck)), "{result:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_block_a_fifth_lane_on_one_session_is_overloaded_and_reads_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(5 * BLOCK_SIZE, 10).await;
    rig.writer.set_durable(DurableFault::Hold);
    let lanes: Vec<_> = (0..4)
        .map(|index| spawn_put(&rig, &upload, index))
        .collect();
    rig.writer.wait_parked(4).await;

    // Act
    let (result, read) = one_more(&rig, &upload, 4).await;

    // Assert
    assert!(matches!(result, Err(UploadError::Overloaded)), "{result:?}");
    assert_eq!(read, 0);
    rig.writer.release();
    join_all(lanes).await;
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_block_an_account_with_thirty_two_blocks_in_flight_is_overloaded() {
    // Arrange: nine sessions may be open, so the 33rd block has a free lane on a session of its own.
    let mut config = transfer_config();
    config.max_open_sessions = 9;
    let rig = Rig::with_config(config).await;
    let mut uploads = Vec::new();
    for seed in 0..9 {
        uploads.push(rig.upload(EIGHT_MIB, 20 + seed).await);
    }
    rig.writer.set_durable(DurableFault::Hold);
    let mut lanes = Vec::new();
    for upload in &uploads[..8] {
        lanes.extend((0..4).map(|index| spawn_put(&rig, upload, index)));
    }
    rig.writer.wait_parked(32).await;

    // Act
    let (result, read) = one_more(&rig, &uploads[8], 0).await;

    // Assert
    assert!(matches!(result, Err(UploadError::Overloaded)), "{result:?}");
    assert_eq!(read, 0);
    rig.writer.release();
    join_all(lanes).await;
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_block_an_exhausted_buffer_is_overloaded_for_every_session() {
    // Arrange: the buffer holds exactly one block.
    let mut config = transfer_config();
    config.max_buffered_block_bytes = TWO_MIB;
    let rig = Rig::with_config(config).await;
    let first = rig.upload(EIGHT_MIB, 30).await;
    let second = rig.upload(EIGHT_MIB, 31).await;
    rig.writer.set_durable(DurableFault::Hold);
    let held = spawn_put(&rig, &first, 0);
    rig.writer.wait_parked(1).await;

    // Act
    let (result, read) = one_more(&rig, &second, 0).await;

    // Assert
    assert!(matches!(result, Err(UploadError::Overloaded)), "{result:?}");
    assert_eq!(read, 0);
    rig.writer.release();
    join_all(vec![held]).await;
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_block_the_ack_waits_for_the_sync_and_the_block_is_not_received_before() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(EIGHT_MIB, 40).await;
    rig.writer.set_durable(DurableFault::Hold);
    let held = spawn_put(&rig, &upload, 0);
    rig.writer.wait_parked(1).await;

    // Act
    let pending_while_held = !held.is_finished();
    let (duplicate, read) = one_more(&rig, &upload, 0).await;
    let (received, _, _) = rig.row(upload.id).await;
    rig.writer.release();
    let acked = held.await.expect("lane task");
    let repeat = rig.put(&upload, 0).await;

    // Assert
    assert!(pending_while_held, "no ack before the sync returns");
    assert!(
        matches!(duplicate, Err(UploadError::Overloaded)),
        "a block in flight is not acknowledged: {duplicate:?}"
    );
    assert_eq!(read, 0);
    assert!(!bit(&received, 0));
    assert!(matches!(acked, Ok(BlockAck)), "{acked:?}");
    assert!(matches!(repeat, Ok(BlockAck)), "{repeat:?}");
    assert_eq!(
        rig.writer.writes(),
        1,
        "the acked block is not written again"
    );
    rig.finish().await;
}

#[tokio::test]
async fn put_block_a_failed_sync_is_a_storage_error_and_a_resend_succeeds() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(EIGHT_MIB, 41).await;
    rig.writer.set_durable(DurableFault::Io);

    // Act
    let failed = rig.put(&upload, 1).await;
    rig.writer.set_durable(DurableFault::None);
    let writes_after_failure = rig.writer.writes();
    let resend = rig.put(&upload, 1).await;

    // Assert
    assert!(matches!(failed, Err(UploadError::Storage(_))), "{failed:?}");
    assert!(matches!(resend, Ok(BlockAck)), "{resend:?}");
    assert_eq!(
        rig.writer.writes(),
        writes_after_failure + 1,
        "the bit was unset, so the resend was written"
    );
    rig.finish().await;
}
