//! PT-04-2 (AC-2): one 5 GiB DS-09 style blob written by 4 lanes of 2 MiB blocks and read back sequentially,
//! encryption on. The read is compared with the source block by block, so a fast wrong read cannot pass.

use std::sync::Arc;
use std::time::Instant;

use contracts::BlobReader;

use crate::load::{BLOCK, commit, host, knob, mb_per_s, new_blob, pool, reader, write_lane};
use crate::support::{MIB, Rig};

const LANES: usize = 4;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-04-2 throughput is measured on REF-SRV only"]
async fn pt_04_2_a_single_stream_writes_and_reads_at_the_target_rates() {
    // Arrange
    let mib = knob("PT_04_2_MIB", 5 * 1024);
    let blocks = (mib as usize * MIB).div_ceil(BLOCK);
    let rig = Rig::new().await;
    let pool = pool();
    let handle = new_blob(&rig.writer, blocks).await;

    // Act: write
    let began = Instant::now();
    let lanes: Vec<_> = (0..LANES)
        .map(|lane| {
            tokio::spawn(write_lane(
                Arc::clone(&rig.writer),
                Arc::clone(&pool),
                handle.clone(),
                lane,
                LANES,
                blocks,
            ))
        })
        .collect();
    for lane in lanes {
        lane.await.expect("lane");
    }
    let write_elapsed = began.elapsed();
    let blob = commit(&rig, &handle).await;

    // Act: read
    let reader = reader(&rig, 16);
    let began = Instant::now();
    let mut stream = reader
        .open(
            blob.blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            None,
        )
        .await
        .expect("open");
    let mut offset = 0;
    let mut mismatched = 0;
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        let bytes = item.expect("chunk verifies");
        let mut at = 0;
        while at < bytes.len() {
            let in_block = offset % BLOCK;
            let take = (BLOCK - in_block).min(bytes.len() - at);
            let expected = &pool[(offset / BLOCK) % pool.len()][in_block..in_block + take];
            if bytes[at..at + take] != *expected {
                mismatched += 1;
            }
            at += take;
            offset += take;
        }
    }
    let read_elapsed = began.elapsed();

    // Assert
    let size = (blocks * BLOCK) as u64;
    let write = mb_per_s(size, write_elapsed);
    let read = mb_per_s(size, read_elapsed);
    println!(
        "PT-04-2: {} MiB, {LANES} lanes of 2 MiB, write {write:.0} MB/s in {write_elapsed:?}, read {read:.0} MB/s in {read_elapsed:?}; {}",
        size / MIB as u64,
        host()
    );
    assert_eq!(
        (offset as u64, mismatched),
        (size, 0),
        "the read equals the source"
    );
    assert!(write >= 300.0, "write {write:.0} MB/s (failure below 112)");
    assert!(read >= 500.0, "read {read:.0} MB/s (failure below 112)");
    drop(reader);
    rig.finish().await;
}
