//! PT-04-3 (AC-3): 400 concurrent `write_block` + `await_durable` loops of 2 MiB for 3 minutes, paced to an
//! offered aggregate of 120 MB/s, so the delivered aggregate can show the 112 MB/s target of 100 sessions x 4
//! lanes (back-to-back loops saturate the disk and measure queueing, not the barrier; description section 12).
//! Halfway the
//! real sync is made to fail for two group flushes through the `sync-hook` barrier; afterwards every
//! acknowledged block must have a sync of its file that started after the block was written.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{BlobWriter, DurableToken};
use sunnycloud_blobstore::{FileBlobWriter, SyncBarrier, SyncHook};
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

use crate::load::{BLOCK, host, knob, mb_per_s, new_blob, percentile, pool, pool_block};
use crate::support::Rig;

/// Blocks per blob before the writer discards it and starts the next, so the volume holds at most 16 MiB per
/// writer.
const BLOB_BLOCKS: usize = 8;

/// One write: the hook stamp taken before `write_block`, the blob, the ack wait, when the wait ended and whether
/// it was acked.
struct Write {
    blob_id: Uuid,
    before: u64,
    wait: Duration,
    ended: Instant,
    acked: bool,
}

async fn writer_loop(
    writer: Arc<FileBlobWriter>,
    hook: Arc<SyncHook>,
    pool: Arc<Vec<bytes::Bytes>>,
    interval: Duration,
    offset: Duration,
    until: Instant,
) -> Vec<Write> {
    let mut ticks = tokio::time::interval_at((Instant::now() + offset).into(), interval);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut writes = Vec::new();
    'blobs: loop {
        let handle = new_blob(&writer, BLOB_BLOCKS).await;
        for index in 0..BLOB_BLOCKS {
            ticks.tick().await;
            if Instant::now() >= until {
                writer.discard(handle.plan.blob_id).await.expect("discard");
                break 'blobs;
            }
            let before = hook.tick();
            let token: DurableToken = writer
                .write_block(&handle, index as u32, pool_block(&pool, index))
                .await
                .expect("block is written");
            let began = Instant::now();
            let acked = writer.await_durable(token).await.is_ok();
            writes.push(Write {
                blob_id: handle.plan.blob_id,
                before,
                wait: began.elapsed(),
                ended: Instant::now(),
                acked,
            });
        }
        writer.discard(handle.plan.blob_id).await.expect("discard");
    }
    writes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-04-3 is measured on REF-SRV only"]
async fn pt_04_3_four_hundred_writers_get_acks_within_target_and_no_unsynced_ack() {
    // Arrange
    let writers = knob("PT_04_3_WRITERS", 400);
    let seconds = knob("PT_04_3_SECONDS", 180);
    let offered = knob("PT_04_3_OFFERED_MBPS", 120) as f64;
    let rig = Rig::new().await;
    let hook = SyncHook::new();
    let writer = Arc::new(
        FileBlobWriter::new(
            &rig.config,
            Arc::clone(&rig.keyring),
            SyncBarrier::with_hook(Duration::ZERO, Arc::clone(&hook)),
        )
        .expect("writer"),
    );
    let pool = pool();
    let interval = Duration::from_secs_f64(writers as f64 * BLOCK as f64 / (offered * 1e6));
    let began = Instant::now();
    let until = began + Duration::from_secs(seconds);

    // Act
    let tasks: Vec<_> = (0..writers)
        .map(|n| {
            tokio::spawn(writer_loop(
                Arc::clone(&writer),
                Arc::clone(&hook),
                Arc::clone(&pool),
                interval,
                interval.mul_f64(n as f64 / writers as f64),
                until,
            ))
        })
        .collect();
    tokio::time::sleep(Duration::from_secs(seconds / 2)).await;
    hook.fail_next_flushes(2);
    let mut writes = Vec::new();
    for task in tasks {
        writes.extend(task.await.expect("writer task"));
    }

    // Assert: delivery is measured to the last acknowledgement; a writer's wait for a tick past the end is idle
    let elapsed = writes
        .iter()
        .map(|write| write.ended)
        .max()
        .map_or(Duration::ZERO, |end| end - began);
    let mut synced: HashMap<Uuid, Vec<u64>> = HashMap::new();
    for record in hook.records() {
        synced.entry(record.blob_id).or_default().push(record.stamp);
    }
    let unsynced = writes
        .iter()
        .filter(|write| write.acked)
        .filter(|write| {
            synced
                .get(&write.blob_id)
                .is_none_or(|stamps| stamps.iter().all(|stamp| *stamp < write.before))
        })
        .count();
    let failed = writes.iter().filter(|write| !write.acked).count();
    let mut waits: Vec<Duration> = writes
        .iter()
        .filter(|write| write.acked)
        .map(|write| write.wait)
        .collect();
    waits.sort_unstable();
    let acked_bytes = waits.len() as u64 * BLOCK as u64;
    let aggregate = mb_per_s(acked_bytes, elapsed);
    let p95 = percentile(&waits, 95);
    println!(
        "PT-04-3: {writers} writers, offered {offered:.0} MB/s, {seconds} s: {} acked, {failed} refused by the failed syncs, \
         {unsynced} acked but unsynced, await_durable p50 {:?} p95 {p95:?} p99 {:?} max {:?}, aggregate {aggregate:.0} MB/s, \
         {} group flushes; {}",
        waits.len(),
        percentile(&waits, 50),
        percentile(&waits, 99),
        waits.last().copied().unwrap_or_default(),
        hook.flush_count(),
        host()
    );
    assert!(failed > 0, "the simulated sync failure reached writers");
    assert_eq!(unsynced, 0, "acknowledged but unsynced blocks");
    assert!(
        p95 <= Duration::from_millis(100),
        "p95 {p95:?} (failure 250 ms)"
    );
    assert!(aggregate >= 112.0, "aggregate {aggregate:.0} MB/s");
    drop(writer);
    rig.finish().await;
}
