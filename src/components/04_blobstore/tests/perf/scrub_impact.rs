//! PT-04-4 (AC-4): a CLI-style scrub (no activity gauge, `rate = 50 MiB/s`) over 20 GiB of committed blobs
//! leaves concurrent 4-lane block writes at least 95% of their baseline, and its own read rate at or below
//! 55 MiB/s. The baseline and the scrubbed phase run the same writes on the same filled volume.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{BlobWriter, ByteRate, ScrubMode, Scrubber};

use crate::load::{
    BLOCK, commit, four_lanes_until, host, knob, mb_per_s, new_blob, pool, pool_block,
};
use crate::support::{MIB, Rig};

const RATE: u64 = 50 * MIB as u64;
const FILL_BLOB_BLOCKS: usize = 32;

/// Committed blobs of 64 MiB until `mib` MiB are stored.
async fn fill(rig: &Rig, mib: u64) {
    let pool = pool();
    let blobs = (mib as usize * MIB).div_ceil(FILL_BLOB_BLOCKS * BLOCK);
    for _ in 0..blobs {
        let handle = new_blob(&rig.writer, FILL_BLOB_BLOCKS).await;
        for index in 0..FILL_BLOB_BLOCKS {
            let token = rig
                .writer
                .write_block(&handle, index as u32, pool_block(&pool, index))
                .await
                .expect("block");
            rig.writer.await_durable(token).await.expect("durable");
        }
        commit(rig, &handle).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-04-4 is measured on REF-SRV only"]
async fn pt_04_4_a_rate_limited_scrub_keeps_writes_near_baseline_and_stays_under_its_cap() {
    // Arrange
    let seconds = knob("PT_04_4_SECONDS", 60);
    let scrub_mib = knob("PT_04_4_SCRUB_MIB", 20 * 1024);
    let rig = Rig::new().await;
    fill(&rig, scrub_mib).await;
    let pool = pool();
    let scrub = rig.scrub(4);
    let probe = scrub.scrubber.read_probe();

    // Act: baseline
    let began = Instant::now();
    let baseline_bytes = four_lanes_until(
        Arc::clone(&rig.writer),
        Arc::clone(&pool),
        began + Duration::from_secs(seconds),
    )
    .await;
    let baseline = mb_per_s(baseline_bytes, began.elapsed());

    // Act: the same writes with the scrub running
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, Some(ByteRate(RATE)))
        .await
        .expect("scrub run");
    let began = Instant::now();
    let until = began + Duration::from_secs(seconds);
    let scrubber = Arc::clone(&scrub.scrubber);
    let scrubbing = tokio::spawn(async move {
        while Instant::now() < until {
            if scrubber.run_step(&run).await.expect("scrub step").done {
                break;
            }
        }
        Instant::now()
    });
    let with_scrub_bytes =
        four_lanes_until(Arc::clone(&rig.writer), Arc::clone(&pool), until).await;
    let with_scrub = mb_per_s(with_scrub_bytes, began.elapsed());
    let scrub_end = scrubbing.await.expect("scrub task");
    let scrub_rate = probe.chunk_bytes() as f64 / MIB as f64 / (scrub_end - began).as_secs_f64();

    // Assert
    let ratio = with_scrub / baseline;
    println!(
        "PT-04-4: {seconds} s phases over {scrub_mib} MiB of blobs: baseline {baseline:.0} MB/s, with scrub {with_scrub:.0} MB/s \
         ({:.1}%), scrub read {scrub_rate:.1} MiB/s at a 50 MiB/s cap; {}",
        ratio * 100.0,
        host()
    );
    assert!(
        ratio >= 0.95,
        "writes at {:.1}% of baseline (failure below 90%)",
        ratio * 100.0
    );
    assert!(
        scrub_rate <= 55.0,
        "scrub {scrub_rate:.1} MiB/s (failure at 60)"
    );
    drop(scrub);
    rig.finish().await;
}
