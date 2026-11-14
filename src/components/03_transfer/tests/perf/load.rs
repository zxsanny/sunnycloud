//! Shared pieces of the PT-03 cases: the perf rig (production transfer and 04 settings, a 16-connection component
//! pool), block sources shaped like DS-25 (dense) and DS-10 (sparse-hybrid), request bodies of zero-copy pieces
//! that count the bytes the service has taken and not yet answered for, a client lane that retries `Overloaded`,
//! and whole-file uploads on four lanes.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use contracts::{
    ByteStream, CommitActor, CommitUpload, Committed, StartUpload, StreamError, UploadError,
    UploadService,
};
use futures_core::Stream;
use sc_db::{Db, DbConfig, DbRole};
use sc_ids::{BLOCK_SIZE, Hash32, hash_block};
use sc_testdb::TestDb;
use sunnycloud_transfer::PgUploadService;
use uuid::Uuid;

use crate::put_support::content;
use crate::start_support::{Rig, started, transfer_config};
use crate::support::Fixture;

/// PT-03-1: at most 16 database connections.
pub const POOL: u32 = 16;
/// 04's production default (`blob_read_ahead_chunks`).
const READ_AHEAD: u64 = 16;
/// The client's lanes per session (`max_lanes`).
pub const LANES: usize = 4;
/// What a client waits after `Overloaded` before it sends the block again (the `Retry-After` of 11).
const RETRY_AFTER: Duration = Duration::from_millis(10);
/// A random block at every 64th index, zeros elsewhere (DS-10).
const SPARSE_STRIDE: u32 = 64;
const PIECE: usize = 256 * 1024;

impl Rig {
    /// The PT-03 rig: the production transfer defaults, 04's production read-ahead and preallocation, and a
    /// component pool of [`POOL`] connections shared by 02, 03, 04 and 07a.
    pub async fn for_perf() -> Rig {
        let db = TestDb::clone_template().await;
        let config = DbConfig {
            db_pool_max: POOL,
            ..db.config()
        };
        let api = Db::connect(&config, DbRole::Api)
            .await
            .expect("connect the measured pool");
        Rig::build_on(Fixture { db, api }, transfer_config(), 0, true, READ_AHEAD).await
    }
}

/// The `pct` percentile of `times`.
pub fn percentile(times: &mut [Duration], pct: usize) -> Duration {
    assert!(!times.is_empty(), "no samples");
    times.sort_unstable();
    times[(times.len() * pct / 100).min(times.len() - 1)]
}

/// The host the numbers were taken on, printed with every result line.
pub fn host() -> String {
    match std::fs::read_to_string("/proc/sys/kernel/hostname") {
        Ok(name) => name.trim().to_owned(),
        Err(error) => format!("unknown host ({error})"),
    }
}

/// Megabytes (10^6) per second.
pub fn mb_per_s(bytes: u64, elapsed: Duration) -> f64 {
    bytes as f64 / 1e6 / elapsed.as_secs_f64()
}

/// Bytes taken from request bodies whose call has not answered yet: an upper bound of what the service holds
/// buffered (it gives a block's bytes back when the block is written, before it answers).
#[derive(Default)]
pub struct Buffered {
    now: AtomicU64,
    peak: AtomicU64,
}

impl Buffered {
    fn take(&self, bytes: u64) {
        let now = self.now.fetch_add(bytes, Ordering::SeqCst) + bytes;
        self.peak.fetch_max(now, Ordering::SeqCst);
    }

    fn give_back(&self, bytes: u64) {
        self.now.fetch_sub(bytes, Ordering::SeqCst);
    }

    pub fn peak(&self) -> u64 {
        self.peak.load(Ordering::SeqCst)
    }
}

/// A body of zero-copy pieces of one buffer; with a counter, every piece taken is added to it and to `buffered`.
struct Pieces {
    bytes: Bytes,
    at: usize,
    counted: Option<(Arc<Buffered>, Arc<AtomicU64>)>,
}

impl Stream for Pieces {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.at >= self.bytes.len() {
            return Poll::Ready(None);
        }
        let end = (self.at + PIECE).min(self.bytes.len());
        let piece = self.bytes.slice(self.at..end);
        self.at = end;
        if let Some((buffered, taken)) = &self.counted {
            buffered.take(piece.len() as u64);
            taken.fetch_add(piece.len() as u64, Ordering::SeqCst);
        }
        Poll::Ready(Some(Ok(piece)))
    }
}

pub fn body(bytes: Bytes) -> ByteStream {
    Box::pin(Pieces {
        bytes,
        at: 0,
        counted: None,
    })
}

fn counted_body(bytes: Bytes, buffered: &Arc<Buffered>) -> (ByteStream, Arc<AtomicU64>) {
    let taken = Arc::new(AtomicU64::new(0));
    let stream = Pieces {
        bytes,
        at: 0,
        counted: Some((Arc::clone(buffered), Arc::clone(&taken))),
    };
    (Box::pin(stream), taken)
}

/// Where the blocks of a generated file come from: `distinct` random blocks reused in turn (DS-25, dense), or the
/// same random blocks at every 64th index and zeros elsewhere (DS-10, sparse-hybrid).
pub struct Blocks {
    random: Vec<(Bytes, Hash32)>,
    zero: Option<(Bytes, Hash32)>,
}

impl Blocks {
    pub fn dense(distinct: u64) -> Blocks {
        Blocks {
            random: random_blocks(distinct),
            zero: None,
        }
    }

    pub fn sparse_hybrid(distinct: u64) -> Blocks {
        let zero = Bytes::from(vec![0u8; BLOCK_SIZE]);
        let hash = hash_block(&zero);
        Blocks {
            random: random_blocks(distinct),
            zero: Some((zero, hash)),
        }
    }

    /// Block `index` of a file of `size` bytes and its hash.
    pub fn block(&self, index: u32, size: u64) -> (Bytes, Hash32) {
        let (bytes, hash) = match &self.zero {
            Some(zero) if !index.is_multiple_of(SPARSE_STRIDE) => zero,
            Some(_) => &self.random[(index / SPARSE_STRIDE) as usize % self.random.len()],
            None => &self.random[index as usize % self.random.len()],
        };
        let start = u64::from(index) * BLOCK_SIZE as u64;
        let len = usize::try_from((size - start).min(BLOCK_SIZE as u64)).expect("block length");
        if len == bytes.len() {
            return (bytes.clone(), *hash);
        }
        let part = bytes.slice(..len);
        let part_hash = hash_block(&part);
        (part, part_hash)
    }

    /// The client's blocklist of a file of `size` bytes.
    pub fn blocklist(&self, size: u64) -> Vec<Hash32> {
        (0..block_count(size))
            .map(|index| self.block(index, size).1)
            .collect()
    }
}

fn random_blocks(distinct: u64) -> Vec<(Bytes, Hash32)> {
    (0..distinct)
        .map(|seed| {
            let bytes = Bytes::from(content(7_000 + seed, BLOCK_SIZE));
            let hash = hash_block(&bytes);
            (bytes, hash)
        })
        .collect()
}

pub fn block_count(size: u64) -> u32 {
    u32::try_from(size.div_ceil(BLOCK_SIZE as u64)).expect("block count")
}

/// One block as a client lane sends it, sent again after each `Overloaded`: the latency of the call that was
/// answered and how many calls were refused as `Overloaded` before it.
pub async fn put(
    service: &PgUploadService,
    account: Uuid,
    upload_id: Uuid,
    index: u32,
    block: &(Bytes, Hash32),
    buffered: &Arc<Buffered>,
) -> Result<(Duration, u64), UploadError> {
    let mut overloaded = 0;
    loop {
        let (stream, taken) = counted_body(block.0.clone(), buffered);
        let began = Instant::now();
        let result = service
            .put_block(
                account,
                upload_id,
                index,
                block.1,
                stream,
                block.0.len() as u64,
            )
            .await;
        let latency = began.elapsed();
        buffered.give_back(taken.load(Ordering::SeqCst));
        match result {
            Ok(_) => return Ok((latency, overloaded)),
            Err(UploadError::Overloaded) => {
                overloaded += 1;
                tokio::time::sleep(RETRY_AFTER).await;
            }
            Err(other) => return Err(other),
        }
    }
}

pub fn start_request(parent: Uuid, name: &str, size: u64) -> StartUpload {
    StartUpload {
        parent_id: parent,
        name: name.to_owned(),
        declared_size: i64::try_from(size).expect("declared size"),
        client_key: name.to_owned(),
        content_hash: None,
        base_revision: None,
        client_meta: None,
    }
}

/// Starts `name` under `parent` and sends every block of a `size`-byte file on [`LANES`] lanes; returns the
/// upload id.
pub async fn send_file(
    service: &Arc<PgUploadService>,
    account: Uuid,
    parent: Uuid,
    name: &str,
    size: u64,
    blocks: &Arc<Blocks>,
) -> Uuid {
    let uploads: &dyn UploadService = service.as_ref();
    let upload_id = started(
        uploads
            .start_upload(account, start_request(parent, name, size))
            .await,
    )
    .upload_id;
    let count = block_count(size);
    let next = Arc::new(AtomicU32::new(0));
    let buffered = Arc::new(Buffered::default());
    let lanes: Vec<_> = (0..LANES)
        .map(|_| {
            let (service, blocks, next, buffered) = (
                Arc::clone(service),
                Arc::clone(blocks),
                Arc::clone(&next),
                Arc::clone(&buffered),
            );
            tokio::spawn(async move {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= count {
                        return;
                    }
                    let block = blocks.block(index, size);
                    put(&service, account, upload_id, index, &block, &buffered)
                        .await
                        .expect("block acknowledged");
                }
            })
        })
        .collect();
    for lane in lanes {
        lane.await.expect("lane");
    }
    upload_id
}

/// `commit_upload` of a sent file as a new file `name` under `parent`.
pub async fn commit_file(
    service: &PgUploadService,
    account: Uuid,
    parent: Uuid,
    name: &str,
    upload_id: Uuid,
    blocklist: Vec<Hash32>,
) -> Committed {
    let actor = CommitActor {
        account_id: account,
        device_label: "perf".to_owned(),
    };
    let request = CommitUpload {
        upload_id,
        parent_id: parent,
        name: name.to_owned(),
        base_revision: None,
        blocklist,
        client_mtime: None,
        client_meta: None,
        import_hint: false,
    };
    service
        .commit_upload(actor, request)
        .await
        .expect("committed")
}

/// Sends and commits a `size`-byte file; returns its node.
pub async fn stored_file(
    service: &Arc<PgUploadService>,
    account: Uuid,
    parent: Uuid,
    name: &str,
    size: u64,
    blocks: &Arc<Blocks>,
) -> Uuid {
    let upload_id = send_file(service, account, parent, name, size, blocks).await;
    commit_file(
        service,
        account,
        parent,
        name,
        upload_id,
        blocks.blocklist(size),
    )
    .await
    .node_id
}
