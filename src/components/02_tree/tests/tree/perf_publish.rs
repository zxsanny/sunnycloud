//! AZ-3719 AC-5 (PT-02-3, hot account) and AC-6 (the batch-publish row of PT-02-4). Both measure time, so they
//! run on REF-SRV only (`cargo test -- --ignored`) and are `NOT-RUN(HW-REF)` elsewhere; PT-02-3's targets are
//! derived and wait for calibration on REF-SRV.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{EntryResult, PublishResult, TreeCommit};
use sc_db::{Db, DbConfig, DbRole};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedStamp};
use sunnycloud_tree::PgTreeService;

use crate::mutation_support::{DEFAULT_QUOTA, Tree};
use crate::publish_support::{actor, new_file};

const WRITERS: usize = 50;
const COMMITS: usize = 1000;
/// PT-02-3: at most 16 database connections.
const HOT_POOL: u32 = 16;
const BATCHES: usize = 20;
const BATCH: usize = 500;

fn p95(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[(times.len() * 95 / 100).min(times.len() - 1)]
}

/// The tree's service on a pool of its own of `pool_max` connections, with the same ports.
async fn service_on_pool(tree: &Tree, pool_max: u32) -> (Db, Arc<PgTreeService>) {
    let config = DbConfig {
        db_pool_max: pool_max,
        ..tree.fixture.db.config()
    };
    let db = Db::connect(&config, DbRole::Api)
        .await
        .expect("connect the measured pool");
    let service = Arc::new(PgTreeService::new(
        db.clone(),
        tree.guard.clone(),
        Arc::new(PgQuotaLedger::new(QuotaConfig {
            default_quota_bytes: DEFAULT_QUOTA,
        })),
        tree.queue.clone(),
        Arc::new(PgFeedStamp),
        InProcessNotifier::new(),
        sc_testdb::clock_at_start(),
    ));
    (db, service)
}

/// PT-02-3 on REF-SRV only: 50 writers make 1,000 single-file commits on one account; at least 200 commits/s
/// (fail 100), p95 at most 250 ms (fail 600 ms), and the feed counter gapless with no duplicate `seq`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-02-3 hot-account commit rate is measured on REF-SRV only"]
async fn pt_02_3_fifty_writers_on_one_account_commit_fast_with_a_gapless_feed() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(COMMITS).await;
    let (db, service) = service_on_pool(&tree, HOT_POOL).await;
    let seq_before = tree.seq().await;
    let (account, root) = (tree.account, tree.root);

    // Act
    let started = Instant::now();
    let mut writers = Vec::with_capacity(WRITERS);
    for (writer, chunk) in blobs.chunks(COMMITS / WRITERS).enumerate() {
        let (service, chunk) = (service.clone(), chunk.to_vec());
        writers.push(tokio::spawn(async move {
            let mut times = Vec::with_capacity(chunk.len());
            for (index, blob) in chunk.into_iter().enumerate() {
                let entry = new_file(root, &format!("w{writer}-{index}.txt"), blob);
                let begun = Instant::now();
                service
                    .publish(actor(account), vec![entry], Vec::new(), sc_testdb::START)
                    .await
                    .expect("commit");
                times.push(begun.elapsed());
            }
            times
        }));
    }
    let mut times = Vec::with_capacity(COMMITS);
    for writer in writers {
        times.extend(writer.await.expect("writer"));
    }
    let elapsed = started.elapsed();
    let connections = tree.fixture.db.open_connections().await;

    // Assert
    let rate = COMMITS as f64 / elapsed.as_secs_f64();
    let latency = p95(times);
    println!("PT-02-3: {rate:.0} commits/s, p95 {latency:?}, {connections} connections");
    assert_eq!(tree.seq().await, seq_before + COMMITS as i64, "no gap");
    let distinct = tree
        .count(&format!(
            "SELECT count(DISTINCT seq) FROM nodes WHERE account_id = '{account}' AND parent_id = '{root}'"
        ))
        .await;
    assert_eq!(distinct, COMMITS as i64, "no duplicate seq");
    assert!(rate >= 100.0, "fail below 100/s: {rate:.0}");
    assert!(
        latency <= Duration::from_millis(600),
        "fail above 600 ms: {latency:?}"
    );
    assert!(rate >= 200.0, "target 200/s: {rate:.0}");
    assert!(
        latency <= Duration::from_millis(250),
        "target 250 ms: {latency:?}"
    );
    assert!(
        connections <= i64::from(HOT_POOL) + 1,
        "pool and probe: {connections}"
    );
    drop((service, db));
    tree.finish().await;
}

/// The batch-publish row of PT-02-4 on REF-SRV only: 20 sequential 500-entry batches (10,000 files), each
/// `publish` timed; p95 at most 1.0 s (fail 2.0 s).
#[tokio::test]
#[ignore = "NOT-RUN(HW-REF): PT-02-4 batch-publish time is measured on REF-SRV only"]
async fn pt_02_4_twenty_batches_of_500_entries_publish_within_the_time_target() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(BATCHES * BATCH).await;

    // Act
    let mut times = Vec::with_capacity(BATCHES);
    for (batch, chunk) in blobs.chunks(BATCH).enumerate() {
        let entries = chunk
            .iter()
            .enumerate()
            .map(|(index, blob)| new_file(tree.root, &format!("b{batch}-{index}.txt"), *blob))
            .collect();
        let begun = Instant::now();
        let result = tree
            .service
            .publish(actor(tree.account), entries, Vec::new(), sc_testdb::START)
            .await
            .expect("batch publish");
        times.push(begun.elapsed());
        let PublishResult::Published(results) = result else {
            panic!("unexpected replay");
        };
        assert!(
            results
                .iter()
                .all(|result| matches!(result, EntryResult::Published { .. })),
            "batch {batch}"
        );
    }

    // Assert
    let latency = p95(times);
    println!("PT-02-4 batch publish: p95 {latency:?}");
    assert!(
        latency <= Duration::from_secs(2),
        "fail above 2.0 s: {latency:?}"
    );
    assert!(
        latency <= Duration::from_secs(1),
        "target 1.0 s: {latency:?}"
    );
    assert_eq!(
        tree.count(&format!(
            "SELECT count(*) FROM nodes WHERE parent_id = '{}'",
            tree.root
        ))
        .await,
        (BATCHES * BATCH) as i64
    );
    tree.finish().await;
}
