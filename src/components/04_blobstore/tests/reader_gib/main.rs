//! AC-7: a 1 GiB blob read without a range, in 1 MiB reads, hashes to the generator's hash and the process's peak
//! RSS grows by at most 32 MiB while it is read. Peak RSS is per process, so this binary holds this one test.
//!
//! The cipher and the hash are unoptimized in a debug build (minutes for 2 GiB of work), so the test runs by
//! default only in a release build: `cargo test --release -p sunnycloud-blobstore --test reader_gib`; a debug
//! build runs it with `-- --ignored`.

#[allow(dead_code)]
#[path = "../reader/support.rs"]
mod support;

use std::fs;

use bytes::Bytes;
use contracts::BlobWriter;
use support::{MIB, Rig, next};

const GIB: usize = 1 << 30;
const BLOCK: usize = 2 * MIB;
const LIMIT_KIB: u64 = 32 * 1024;

/// The block of the generator: dense words from a per-block seed.
fn generated(index: usize) -> Vec<u8> {
    let mut state = (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut block = Vec::with_capacity(BLOCK);
    while block.len() < BLOCK {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        block.extend_from_slice(&state.to_le_bytes());
    }
    block
}

fn status_kib(field: &str) -> u64 {
    let status = fs::read_to_string("/proc/self/status").expect("proc status");
    let line = status
        .lines()
        .find(|line| line.starts_with(field))
        .expect("status field");
    line.split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .expect("kib value")
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    debug_assertions,
    ignore = "1 GiB through the unoptimized cipher; run with --release"
)]
async fn a_one_gib_blob_streams_to_the_generator_hash_within_32_mib_of_peak_growth() {
    // Arrange
    let rig = Rig::new().await;
    let plan = rig.writer.plan(rig.account, GIB as i64).expect("plan");
    let handle = rig.writer.create_file(plan, false).await.expect("create");
    let mut manifest = blake3::Hasher::new();
    for index in 0..GIB / BLOCK {
        let block = generated(index);
        manifest.update(&block);
        let token = rig
            .writer
            .write_block(&handle, index as u32, Bytes::from(block))
            .await
            .expect("block is written");
        rig.writer.await_durable(token).await.expect("durable");
    }
    let blob = rig.commit(handle, GIB as i64).await;
    fs::write("/proc/self/clear_refs", "5").expect("reset the peak");
    let baseline = status_kib("VmRSS:");

    // Act
    let mut stream = rig.open(&blob, None).await.expect("open");
    let mut hasher = blake3::Hasher::new();
    let mut pending: Vec<u8> = Vec::with_capacity(MIB);
    let mut total = 0;
    while let Some(item) = next(&mut stream).await {
        pending.extend_from_slice(&item.expect("verified chunk"));
        if pending.len() >= MIB {
            hasher.update(&pending);
            total += pending.len();
            pending.clear();
        }
    }
    hasher.update(&pending);
    total += pending.len();
    let growth = status_kib("VmHWM:").saturating_sub(baseline);

    // Assert
    assert_eq!(total, GIB);
    assert_eq!(hasher.finalize(), manifest.finalize());
    assert!(growth <= LIMIT_KIB, "peak RSS grew by {growth} KiB");
    rig.finish().await;
}
