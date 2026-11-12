//! PT-04-1 (AC-1): random 4 KiB range reads of a 1 GiB encrypted blob from 16 tasks, warm and cold. Cold evicts
//! the blob file from the page cache with `fadvise(DONTNEED)` (per file, no root); a whole-host `drop_caches`
//! is an `e2e-ctl` action of the system run (NFT-PERF-11).

use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{BlobRef, BlobWriter, ByteRange};
use rustix::fs::{Advice, fadvise};
use sunnycloud_blobstore::FileBlobReader;

use crate::load::{BLOCK, commit, host, knob, percentile, pool, pool_block, reader};
use crate::support::{MIB, Rig};

const TASKS: u64 = 16;
const READ: u64 = 4096;

struct Run {
    p95: Duration,
    errors: u64,
    reads: u64,
}

async fn stored_gib(rig: &Rig, mib: u64) -> BlobRef {
    let pool = pool();
    let blocks = (mib as usize * MIB).div_ceil(BLOCK);
    let plan = rig
        .writer
        .plan(rig.account, (blocks * BLOCK) as i64)
        .expect("plan");
    let handle = rig.writer.create_file(plan, false).await.expect("create");
    for index in 0..blocks {
        let token = rig
            .writer
            .write_block(&handle, index as u32, pool_block(&pool, index))
            .await
            .expect("block");
        rig.writer.await_durable(token).await.expect("durable");
    }
    commit(rig, &handle).await
}

async fn random_reads(reader: Arc<FileBlobReader>, blob: Arc<BlobRef>, reads: u64) -> Run {
    let tasks: Vec<_> = (0..TASKS)
        .map(|task| {
            let reader = Arc::clone(&reader);
            let blob = Arc::clone(&blob);
            tokio::spawn(async move {
                let mut state = (task + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
                let mut latencies = Vec::new();
                let mut errors = 0;
                for _ in 0..reads / TASKS {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let start = state % (blob.size as u64 - READ);
                    let began = Instant::now();
                    let read = read_range(&reader, &blob, start, READ).await;
                    latencies.push(began.elapsed());
                    if read != Some(READ as usize) {
                        errors += 1;
                    }
                }
                (latencies, errors)
            })
        })
        .collect();
    let mut latencies = Vec::new();
    let mut errors = 0;
    for task in tasks {
        let (part, failed) = task.await.expect("reader task");
        latencies.extend(part);
        errors += failed;
    }
    latencies.sort_unstable();
    Run {
        p95: percentile(&latencies, 95),
        errors,
        reads: latencies.len() as u64,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-04-1 timing is measured on REF-SRV only"]
async fn pt_04_1_random_4_kib_ranges_meet_the_p95_targets_warm_and_cold() {
    // Arrange
    let mib = knob("PT_04_1_MIB", 1024);
    let reads = knob("PT_04_1_READS", 100_000);
    let rig = Rig::new().await;
    let blob = Arc::new(stored_gib(&rig, mib).await);
    let reader = Arc::new(reader(&rig, 16));
    random_reads(Arc::clone(&reader), Arc::clone(&blob), TASKS * 64).await;

    // Act
    let warm = random_reads(Arc::clone(&reader), Arc::clone(&blob), reads).await;
    let file = File::open(rig.path(blob.blob_id)).expect("blob file");
    file.sync_all().expect("clean pages");
    fadvise(&file, 0, None, Advice::DontNeed).expect("evict the blob file");
    let cold = random_reads(Arc::clone(&reader), Arc::clone(&blob), reads).await;

    // Assert
    println!(
        "PT-04-1: {mib} MiB blob, {TASKS} tasks, warm {} reads p95 {:?} errors {}, cold {} reads p95 {:?} errors {}; {}",
        warm.reads,
        warm.p95,
        warm.errors,
        cold.reads,
        cold.p95,
        cold.errors,
        host()
    );
    assert_eq!(warm.errors + cold.errors, 0, "error rate 0%");
    assert!(
        warm.p95 <= Duration::from_millis(2),
        "warm p95 {:?}",
        warm.p95
    );
    assert!(
        cold.p95 <= Duration::from_millis(4),
        "cold p95 {:?}",
        cold.p95
    );
    drop(reader);
    rig.finish().await;
}

/// Bytes of one range read, `None` on any error.
pub async fn read_range(
    reader: &FileBlobReader,
    blob: &BlobRef,
    start: u64,
    len: u64,
) -> Option<usize> {
    let range = ByteRange {
        start,
        end: start + len - 1,
    };
    let mut stream = contracts::BlobReader::open(
        reader,
        blob.blob_id,
        &blob.wrapped_key,
        blob.master_key_id,
        blob.size,
        Some(range),
    )
    .await
    .ok()?;
    let mut total = 0;
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        total += item.ok()?.len();
    }
    Some(total)
}
