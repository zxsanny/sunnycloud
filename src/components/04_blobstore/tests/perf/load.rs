//! Load helpers of the performance suite: the knobs, the host line, the DS-09 style block pool, a lane writer
//! and the percentile.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use contracts::{
    BlobCatalog as _, BlobHandle, BlobRef, BlobRegistry as _, BlobWriter, RegisterState,
    WrittenBlob, WrittenOrPlanned,
};
use sc_db::Scope;
use sunnycloud_blobstore::{BlobstoreConfig, FileBlobReader, FileBlobWriter};
use uuid::Uuid;

use crate::support::{MIB, Rig};

pub const BLOCK: usize = 2 * MIB;
const POOL: usize = 16;

/// The number in `variable`, or `default` when it is unset.
pub fn knob(variable: &str, default: u64) -> u64 {
    std::env::var(variable).map_or(default, |text| {
        text.parse()
            .unwrap_or_else(|_| panic!("{variable} must be a number, got {text}"))
    })
}

/// Host, filesystem of the blob root and the gate status, printed with every result.
pub fn host() -> String {
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split(':').nth(1))
                .map(|name| name.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown cpu".to_owned());
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let filesystem = match rustix::fs::statfs(std::env::temp_dir()).map(|fs| fs.f_type as u64) {
        Ok(0x5846_5342) => "xfs".to_owned(),
        Ok(0xEF53) => "ext4".to_owned(),
        Ok(0x0102_1994) => "tmpfs".to_owned(),
        Ok(other) => format!("fs 0x{other:x}"),
        Err(error) => format!("statfs failed: {error}"),
    };
    format!(
        "host: {cpu}, {cores} threads, blob root on {filesystem}; gate NOT-RUN(HW-REF) unless this host is REF-SRV"
    )
}

/// 16 dense 2 MiB blocks carrying the DS-09 marker at every MiB; block `i` of a blob is `pool[i % 16]`.
pub fn pool() -> Arc<Vec<Bytes>> {
    Arc::new(
        (0..POOL)
            .map(|index| {
                let mut state = (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
                let mut block = Vec::with_capacity(BLOCK);
                while block.len() < BLOCK {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    block.extend_from_slice(&state.to_le_bytes());
                }
                for mib in [0, MIB] {
                    let marker = format!("SUNNYCLOUD-MARKER-20261103-{index}");
                    block[mib..mib + marker.len()].copy_from_slice(marker.as_bytes());
                }
                Bytes::from(block)
            })
            .collect(),
    )
}

pub fn pool_block(pool: &[Bytes], index: usize) -> Bytes {
    pool[index % pool.len()].clone()
}

pub fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

pub fn mb_per_s(bytes: u64, elapsed: Duration) -> f64 {
    bytes as f64 / 1e6 / elapsed.as_secs_f64()
}

/// Registers the fully written blob of `handle` committed for the rig's account; returns its reference.
pub async fn commit(rig: &Rig, handle: &BlobHandle) -> BlobRef {
    let written = WrittenOrPlanned::Written(WrittenBlob {
        plan: handle.plan.clone(),
        size: handle.plaintext_size,
    });
    rig.api
        .run(Scope::Account(rig.account), async |tx| {
            rig.registry
                .register_in_tx(tx, rig.account, written.clone(), RegisterState::Committed)
                .await
        })
        .await
        .expect("blob row");
    rig.catalog
        .get_ref(rig.account, handle.plan.blob_id)
        .await
        .expect("catalog lookup")
        .expect("committed blob has a reference")
}

/// A reader of the rig's volume with the given read-ahead (production default 16 chunks).
pub fn reader(rig: &Rig, read_ahead: u64) -> FileBlobReader {
    let config = BlobstoreConfig {
        blob_read_ahead_chunks: read_ahead,
        ..rig.config.clone()
    };
    FileBlobReader::new(
        &config,
        Arc::clone(&rig.keyring),
        rig.api.clone(),
        rig.audit.clone(),
        rig.clock.clone(),
    )
    .expect("reader")
}

pub async fn new_blob(writer: &FileBlobWriter, blocks: usize) -> BlobHandle {
    let plan = writer
        .plan(Uuid::new_v4(), (blocks * BLOCK) as i64)
        .expect("plan");
    writer
        .create_file(plan, (blocks * BLOCK) as i64, false)
        .await
        .expect("create_file")
}

/// Writes the blocks `lane, lane + lanes, ...` of `handle`, each acknowledged before the next.
pub async fn write_lane(
    writer: Arc<FileBlobWriter>,
    pool: Arc<Vec<Bytes>>,
    handle: BlobHandle,
    lane: usize,
    lanes: usize,
    blocks: usize,
) {
    for index in (lane..blocks).step_by(lanes) {
        let token = writer
            .write_block(&handle, index as u32, pool_block(&pool, index))
            .await
            .expect("block is written");
        writer.await_durable(token).await.expect("block is durable");
    }
}

/// Four lanes, each writing its own 64 MiB blobs back to back (a blob is discarded once full) until `until`;
/// returns the bytes acknowledged.
pub async fn four_lanes_until(
    writer: Arc<FileBlobWriter>,
    pool: Arc<Vec<Bytes>>,
    until: Instant,
) -> u64 {
    const BLOCKS: usize = 32;
    let lanes: Vec<_> = (0..4)
        .map(|_| {
            let writer = Arc::clone(&writer);
            let pool = Arc::clone(&pool);
            tokio::spawn(async move {
                let mut acked = 0;
                while Instant::now() < until {
                    let handle = new_blob(&writer, BLOCKS).await;
                    for index in 0..BLOCKS {
                        if Instant::now() >= until {
                            break;
                        }
                        let token = writer
                            .write_block(&handle, index as u32, pool_block(&pool, index))
                            .await
                            .expect("block is written");
                        writer.await_durable(token).await.expect("block is durable");
                        acked += BLOCK as u64;
                    }
                    writer.discard(handle.plan.blob_id).await.expect("discard");
                }
                acked
            })
        })
        .collect();
    let mut total = 0;
    for lane in lanes {
        total += lane.await.expect("lane");
    }
    total
}
