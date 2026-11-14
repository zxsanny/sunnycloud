//! `get_upload` and the lazy load (AZ-3736 AC-1, AC-2; IT-03-4 cases A and B): after a connection drop only the
//! in-flight blocks are needed again; after a restart the session is loaded from its checkpoint row on first
//! access and the acknowledged but uncheckpointed blocks are needed again. The resend is measured and printed.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{BlockAck, ByteStream, StreamError, UploadError, UploadState};
use futures_core::Stream;
use sc_ids::{BLOCK_SIZE, hash_block};
use tokio::sync::{mpsc, watch};

use crate::put_support::{Upload, bit};
use crate::start_support::{MIB, Rig};

const HUNDRED_MIB: usize = 100 * MIB as usize;
const LANES: usize = 4;
/// AC-3 as changed 2026-11-11 (G-03-1, AZ-3731): the bound IT-03-4 case B asserts.
const RESEND_BOUND: u64 = 16 << 20;

type Wait = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A body that hands out its first half, reports that on `half_sent`, then fails once `cut` turns true: a
/// connection dropped mid-stream.
struct CutBody {
    first: Option<Bytes>,
    half_sent: mpsc::UnboundedSender<u32>,
    index: u32,
    cut: Wait,
}

impl Stream for CutBody {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first) = self.first.take() {
            self.half_sent.send(self.index).expect("test listens");
            return Poll::Ready(Some(Ok(first)));
        }
        match self.cut.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Some(Err(StreamError::Source(Box::new(
                std::io::Error::from(std::io::ErrorKind::ConnectionReset),
            ))))),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn cut_body(
    block: &[u8],
    index: u32,
    half_sent: &mpsc::UnboundedSender<u32>,
    cut: &watch::Receiver<bool>,
) -> ByteStream {
    let mut cut = cut.clone();
    Box::pin(CutBody {
        first: Some(Bytes::copy_from_slice(&block[..block.len() / 2])),
        half_sent: half_sent.clone(),
        index,
        cut: Box::pin(async move {
            cut.wait_for(|cut| *cut).await.expect("cut sender kept");
        }),
    })
}

/// Sends `blocks` dealt round-robin to four concurrent lanes, each lane one block at a time; a busy lane retries.
pub async fn send_lanes(rig: &Rig, upload: &Upload, blocks: &[u32]) {
    let data = Arc::new(upload.data.clone());
    let mut tasks = Vec::new();
    for lane in 0..LANES {
        let mine: Vec<u32> = blocks.iter().copied().skip(lane).step_by(LANES).collect();
        let (service, account, id) = (Arc::clone(&rig.service), rig.account, upload.id);
        let data = Arc::clone(&data);
        tasks.push(tokio::spawn(async move {
            for index in mine {
                let start = index as usize * BLOCK_SIZE;
                let block = &data[start..(start + BLOCK_SIZE).min(data.len())];
                loop {
                    let (stream, _) = crate::put_support::body(block, 256 * 1024);
                    let hash = hash_block(block);
                    let length = block.len() as u64;
                    match service
                        .put_block(account, id, index, hash, stream, length)
                        .await
                    {
                        Ok(BlockAck) => break,
                        Err(UploadError::Overloaded) => tokio::task::yield_now().await,
                        Err(other) => panic!("block {index}: {other:?}"),
                    }
                }
            }
        }));
    }
    for task in tasks {
        task.await.expect("lane task");
    }
}

async fn state(rig: &Rig, upload: &Upload) -> UploadState {
    rig.service
        .get_upload(rig.account, upload.id)
        .await
        .expect("session state")
}

/// Bytes the client sends again for `blocks`: 2 MiB each, the last block of the file shorter.
fn bytes_of(upload: &Upload, blocks: &[u32]) -> u64 {
    blocks
        .iter()
        .map(|index| upload.block(*index).len() as u64)
        .sum()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_upload_after_a_drop_needs_only_the_cut_in_flight_blocks() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(HUNDRED_MIB, 70).await;
    let cut_blocks = [7u32, 19, 33, 49];
    let sent: Vec<u32> = (0..upload.block_count())
        .filter(|index| !cut_blocks.contains(index))
        .collect();
    send_lanes(&rig, &upload, &sent).await;
    let (half_tx, mut half_rx) = mpsc::unbounded_channel();
    let (cut_tx, cut_rx) = watch::channel(false);
    let mut in_flight = Vec::new();
    for index in cut_blocks {
        let block = upload.block(index);
        let stream = cut_body(block, index, &half_tx, &cut_rx);
        let (service, account, id) = (Arc::clone(&rig.service), rig.account, upload.id);
        let (hash, length) = (upload.hash(index), block.len() as u64);
        in_flight.push(tokio::spawn(async move {
            service
                .put_block(account, id, index, hash, stream, length)
                .await
        }));
    }
    for _ in cut_blocks {
        half_rx.recv().await.expect("a lane is mid-stream");
    }

    // Act
    cut_tx.send(true).expect("lanes listen");
    let mut cut_answers = Vec::new();
    for task in in_flight {
        cut_answers.push(task.await.expect("lane task"));
    }
    let after_drop = state(&rig, &upload).await;
    let resend = bytes_of(&upload, &after_drop.need_blocks);
    println!("IT-03-4 case A: measured resend {resend} bytes");
    send_lanes(&rig, &upload, &after_drop.need_blocks).await;
    let complete = state(&rig, &upload).await;

    // Assert
    for answer in cut_answers {
        assert!(
            matches!(answer, Err(UploadError::LengthMismatch)),
            "{answer:?}"
        );
    }
    assert_eq!(after_drop.need_blocks, cut_blocks);
    assert_eq!(
        after_drop.received_count, 46,
        "the progress entry is intact"
    );
    assert!(resend <= 8 << 20, "resend {resend} bytes");
    assert!(complete.need_blocks.is_empty());
    assert_eq!(complete.received_count, 50);
    assert!(rig.read_back(&upload).await == upload.data, "file intact");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn get_upload_after_a_restart_loads_the_checkpoint_and_relists_uncheckpointed_blocks() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(HUNDRED_MIB, 71).await;
    let acked: Vec<u32> = (0..30).collect();
    send_lanes(&rig, &upload, &acked).await;
    let (checkpoint, hashes, _) = rig.row(upload.id).await;

    // Act
    rig.restart();
    let loaded = state(&rig, &upload).await;
    let relisted: Vec<u32> = acked
        .iter()
        .copied()
        .filter(|index| loaded.need_blocks.contains(index))
        .collect();
    let resend = bytes_of(&upload, &relisted);
    println!(
        "IT-03-4 case B: measured resend {resend} bytes ({} blocks acked before the restart, not checkpointed)",
        relisted.len()
    );
    send_lanes(&rig, &upload, &loaded.need_blocks).await;
    let complete = state(&rig, &upload).await;

    // Assert
    let from_row: Vec<u32> = (0..upload.block_count())
        .filter(|index| !bit(&checkpoint, *index))
        .collect();
    assert_eq!(
        loaded.need_blocks, from_row,
        "loaded from the checkpoint row"
    );
    assert_eq!(
        loaded.received_count,
        upload.block_count() - u32::try_from(from_row.len()).expect("count")
    );
    for index in (0..upload.block_count()).filter(|index| bit(&checkpoint, *index)) {
        let slot = index as usize * 32;
        assert_eq!(&hashes[slot..slot + 32], upload.hash(index).as_bytes());
    }
    assert!(
        resend <= RESEND_BOUND,
        "resend {resend} bytes over the 16 MiB bound"
    );
    assert!(complete.need_blocks.is_empty());
    assert!(
        rig.read_back(&upload).await == upload.data,
        "blocks written before and after the restart"
    );
    rig.finish().await;
}

#[tokio::test]
async fn put_block_is_a_first_access_after_a_restart() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(16 << 20, 72).await;
    for index in 0..3 {
        rig.put(&upload, index)
            .await
            .expect("block before the restart");
    }

    // Act
    rig.restart();
    let first = rig.put(&upload, 3).await;
    let after = state(&rig, &upload).await;

    // Assert
    assert!(matches!(first, Ok(BlockAck)), "{first:?}");
    assert_eq!(
        after.need_blocks,
        [0, 1, 2, 4, 5, 6, 7],
        "6 MiB acked before the restart were never checkpointed"
    );
    assert_eq!(after.received_count, 1);
    rig.finish().await;
}
