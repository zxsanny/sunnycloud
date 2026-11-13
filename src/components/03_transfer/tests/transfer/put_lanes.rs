//! `put_block` AC-6 (IT-03-20, AC-7): four lanes sending the blocks of DS-34-sized slices (4 MiB + 1 B to
//! 48 MiB) in random order with 10% duplicates assemble the right file; identical duplicates never conflict.
//! A duplicate that meets its original still in flight is told to retry (`Overloaded`), as a client would.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use contracts::{BlockAck, UploadError};
use sc_ids::{BLOCK_SIZE, Hash32, hash_block, rollup};

use crate::put_support::{Upload, body};
use crate::start_support::Rig;

const LANES: usize = 4;
const SEED: u64 = 0x5EED_0034;
const SLICES: [usize; 4] = [
    4 * BLOCK_SIZE / 2 + 1,
    7 * (1 << 20) + 5,
    16 * (1 << 20),
    48 * (1 << 20),
];

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).expect("31 bits") % bound
    }
}

/// Every block once plus 10% (at least one) a second time, shuffled, dealt to the lanes in turn.
fn lanes(blocks: u32, random: &mut Lcg) -> Vec<Vec<u32>> {
    let mut operations: Vec<u32> = (0..blocks).collect();
    let duplicates = (blocks as usize).div_ceil(10);
    for _ in 0..duplicates {
        let index = u32::try_from(random.below(blocks as usize)).expect("index");
        operations.push(index);
    }
    for position in (1..operations.len()).rev() {
        operations.swap(position, random.below(position + 1));
    }
    let mut dealt = vec![Vec::new(); LANES];
    for (position, index) in operations.into_iter().enumerate() {
        dealt[position % LANES].push(index);
    }
    dealt
}

/// Sends the lanes concurrently; returns the `BlockConflict` answers counted.
async fn send(rig: &Rig, upload: &Upload, lanes: Vec<Vec<u32>>) -> usize {
    let conflicts = Arc::new(AtomicUsize::new(0));
    let data = Arc::new(upload.data.clone());
    let mut tasks = Vec::new();
    for lane in lanes {
        let (service, account, id) = (Arc::clone(&rig.service), rig.account, upload.id);
        let (data, conflicts) = (Arc::clone(&data), Arc::clone(&conflicts));
        tasks.push(tokio::spawn(async move {
            for index in lane {
                let start = index as usize * BLOCK_SIZE;
                let block = &data[start..(start + BLOCK_SIZE).min(data.len())];
                loop {
                    let (stream, _) = body(block, 256 * 1024);
                    let length = block.len() as u64;
                    let hash = hash_block(block);
                    match service
                        .put_block(account, id, index, hash, stream, length)
                        .await
                    {
                        Ok(BlockAck) => break,
                        Err(UploadError::Overloaded) => tokio::task::yield_now().await,
                        Err(UploadError::BlockConflict) => {
                            conflicts.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                        Err(other) => panic!("block {index}: {other:?}"),
                    }
                }
            }
        }));
    }
    for task in tasks {
        task.await.expect("lane task");
    }
    conflicts.load(Ordering::SeqCst)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_block_four_racing_lanes_assemble_every_slice() {
    // Arrange
    let rig = Rig::new().await;
    let mut random = Lcg(SEED);
    let mut uploads = Vec::new();
    for (seed, size) in SLICES.iter().enumerate() {
        uploads.push(rig.upload(*size, 340 + seed as u64).await);
    }

    for upload in &uploads {
        let manifest: Vec<Hash32> = (0..upload.block_count()).map(|i| upload.hash(i)).collect();
        let dealt = lanes(upload.block_count(), &mut random);

        // Act
        let conflicts = send(&rig, upload, dealt).await;
        let writes = rig.writer.writes();
        let mut repeats = Vec::new();
        for index in 0..upload.block_count() {
            repeats.push(
                rig.put(upload, index)
                    .await
                    .map_err(|error| error.to_string()),
            );
        }
        let stored = rig.read_back(upload).await;
        let stored_hashes: Vec<Hash32> = stored.chunks(BLOCK_SIZE).map(hash_block).collect();

        // Assert
        let size = upload.data.len();
        assert_eq!(
            conflicts, 0,
            "slice {size}: identical duplicates never conflict"
        );
        assert_eq!(
            repeats,
            vec![Ok(BlockAck); manifest.len()],
            "slice {size}: every block is received with its manifest hash"
        );
        assert_eq!(
            rig.writer.writes(),
            writes,
            "slice {size}: repeats write nothing"
        );
        assert!(
            stored == upload.data,
            "slice {size}: file bytes equal the source"
        );
        assert_eq!(rollup(&stored_hashes), rollup(&manifest), "slice {size}");
    }
    rig.finish().await;
}
