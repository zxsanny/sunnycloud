//! PT-03-1 (AZ-3759 AC-2; AC-2, AC-38): block ingest at 03's boundary with the real fdatasync path, no network.
//! One session on four lanes for three minutes (throughput), then 100 sessions on four lanes each for three minutes
//! (ack latency). 25 accounts hold four sessions each, so every account stays inside its 8 open sessions and 32
//! blocks in flight. A session of 256 MiB that is sent in full is aborted and the lanes go on with a new one, so
//! the preallocated files stay at about 25 GiB.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use contracts::UploadService;
use sc_ids::BLOCK_SIZE;
use sunnycloud_transfer::PgUploadService;
use uuid::Uuid;

use crate::load::{Blocks, Buffered, LANES, POOL, host, mb_per_s, percentile, put, start_request};
use crate::start_support::{Rig, started, transfer_config};

const DURATION: Duration = Duration::from_secs(180);
const SESSIONS: usize = 100;
const SESSIONS_PER_ACCOUNT: usize = 4;
const SESSION_BLOCKS: u32 = 128;
const SAMPLE_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Tally {
    latencies: Vec<Duration>,
    bytes: u64,
    overloaded: u64,
    errors: u64,
    first_error: Option<String>,
}

impl Tally {
    fn merge(&mut self, other: Tally) {
        self.latencies.extend(other.latencies);
        self.bytes += other.bytes;
        self.overloaded += other.overloaded;
        self.errors += other.errors;
        if self.first_error.is_none() {
            self.first_error = other.first_error;
        }
    }
}

/// What every lane of the run shares.
#[derive(Clone)]
struct Load {
    service: Arc<PgUploadService>,
    blocks: Arc<Blocks>,
    buffered: Arc<Buffered>,
    deadline: Instant,
}

async fn lane(load: Load, account: Uuid, upload_id: Uuid, next: Arc<AtomicU32>) -> Tally {
    let mut tally = Tally::default();
    let size = u64::from(SESSION_BLOCKS) * BLOCK_SIZE as u64;
    while Instant::now() < load.deadline {
        let index = next.fetch_add(1, Ordering::SeqCst);
        if index >= SESSION_BLOCKS {
            break;
        }
        let block = load.blocks.block(index, size);
        match put(
            &load.service,
            account,
            upload_id,
            index,
            &block,
            &load.buffered,
        )
        .await
        {
            Ok((latency, refused)) => {
                tally.latencies.push(latency);
                tally.bytes += block.0.len() as u64;
                tally.overloaded += refused;
            }
            Err(error) => {
                tally.errors += 1;
                tally.first_error.get_or_insert(format!("{error:?}"));
            }
        }
    }
    tally
}

/// One client session after another on `LANES` lanes until the deadline.
async fn sessions(load: Load, account: Uuid, root: Uuid, worker: usize) -> Tally {
    let mut tally = Tally::default();
    let size = u64::from(SESSION_BLOCKS) * BLOCK_SIZE as u64;
    let uploads: &dyn UploadService = load.service.as_ref();
    let mut round = 0;
    while Instant::now() < load.deadline {
        let name = format!("ingest-{worker}-{round}.bin");
        round += 1;
        let upload_id = started(
            uploads
                .start_upload(account, start_request(root, &name, size))
                .await,
        )
        .upload_id;
        let next = Arc::new(AtomicU32::new(0));
        let lanes: Vec<_> = (0..LANES)
            .map(|_| tokio::spawn(lane(load.clone(), account, upload_id, Arc::clone(&next))))
            .collect();
        for handle in lanes {
            tally.merge(handle.await.expect("lane"));
        }
        uploads
            .abort_upload(account, upload_id)
            .await
            .expect("abort the sent session");
    }
    tally
}

/// Runs one worker per `(account, root)` for [`DURATION`]; returns the tally, the wall time and the most
/// connections the test database had at a sample.
async fn run(rig: &Rig, load: &Load, owners: &[(Uuid, Uuid)]) -> (Tally, Duration, i64) {
    let started_at = Instant::now();
    let workers: Vec<_> = owners
        .iter()
        .enumerate()
        .map(|(worker, (account, root))| {
            tokio::spawn(sessions(load.clone(), *account, *root, worker))
        })
        .collect();
    let mut connections = 0;
    while !workers.iter().all(|worker| worker.is_finished()) {
        connections = connections.max(rig.fixture.db.open_connections().await);
        tokio::time::sleep(SAMPLE_EVERY).await;
    }
    let mut tally = Tally::default();
    for worker in workers {
        tally.merge(worker.await.expect("worker"));
    }
    (tally, started_at.elapsed(), connections)
}

/// PT-03-1 on REF-SRV only: single-session throughput at least 250 MB/s (fail 112 MB/s); ack p95 at 100 sessions
/// at most 150 ms (fail 400 ms); no error but the intended `Overloaded`; buffered bytes within
/// `max_buffered_block_bytes`; at most 16 database connections.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-03-1 ingest throughput and ack latency are measured on REF-SRV only"]
async fn pt_03_1_block_ingest_meets_throughput_and_ack_latency() {
    // Arrange
    let rig = Rig::for_perf().await;
    let max_buffered = transfer_config().max_buffered_block_bytes;
    let blocks = Arc::new(Blocks::dense(64));
    let mut owners = Vec::with_capacity(SESSIONS);
    for account in 0..SESSIONS / SESSIONS_PER_ACCOUNT {
        let (id, root) = if account == 0 {
            (rig.account, rig.root)
        } else {
            rig.other_account().await
        };
        owners.extend(std::iter::repeat_n((id, root), SESSIONS_PER_ACCOUNT));
    }
    let load = |buffered: &Arc<Buffered>| Load {
        service: Arc::clone(&rig.service),
        blocks: Arc::clone(&blocks),
        buffered: Arc::clone(buffered),
        deadline: Instant::now() + DURATION,
    };
    let (single_buffered, many_buffered) = (Arc::default(), Arc::default());

    // Act
    let (single, single_time, single_connections) =
        run(&rig, &load(&single_buffered), &owners[..1]).await;
    let (many, _, many_connections) = run(&rig, &load(&many_buffered), &owners).await;

    // Assert
    let throughput = mb_per_s(single.bytes, single_time);
    let mut latencies = many.latencies;
    let ack_p95 = percentile(&mut latencies, 95);
    let buffered = single_buffered.peak().max(many_buffered.peak());
    let connections = single_connections.max(many_connections);
    println!(
        "PT-03-1 on {}: single {throughput:.0} MB/s; 100 sessions ack p95 {ack_p95:?}, {} MB acked, \
         {} Overloaded; errors {} + {}; buffered peak {buffered} B; {connections} connections",
        host(),
        many.bytes / 1_000_000,
        many.overloaded,
        single.errors,
        many.errors
    );
    assert_eq!(
        (single.errors, many.errors),
        (0, 0),
        "first error: {:?}",
        single.first_error.or(many.first_error)
    );
    assert!(buffered <= max_buffered, "buffered {buffered} B");
    assert!(connections <= i64::from(POOL), "{connections} connections");
    assert!(throughput >= 112.0, "fail below 112 MB/s: {throughput:.0}");
    assert!(
        ack_p95 <= Duration::from_millis(400),
        "fail above 400 ms: {ack_p95:?}"
    );
    assert!(throughput >= 250.0, "target 250 MB/s: {throughput:.0}");
    assert!(
        ack_p95 <= Duration::from_millis(150),
        "target 150 ms: {ack_p95:?}"
    );
    rig.finish().await;
}
