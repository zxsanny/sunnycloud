//! AZ-3711 AC-4 (IT-02-15, NFT-RES-38: 1,000 concurrent reserves with publishes, deletes and cascade batches on
//! one account keep usage exact and under the quota) and its AC-5 variant (an entry of a reaped session fails
//! inside the workload and counts nothing). The workload runs on a pool of its own, larger than the fixture's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use contracts::{
    CommitEffect, EffectError, EntryResult, PublishEntry, PublishResult, QuotaAdmin, QuotaLedger,
    QuotaLedgerError, TreeCommit, TreeMaintenance,
};
use sc_db::{Db, DbConfig, DbRole, DbTx, Scope};
use sc_error::ErrorCode;
use sunnycloud_quota::PgQuotaLedger;
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedHorizon, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::{PgTreeMaintenance, PgTreeService};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::maintenance_support::{config, ledger};
use crate::mutation_support::Tree;
use crate::publish_support::{TestEffect, actor, effects};
use crate::support::START;
use crate::usage_support::{KIB, MIB, edit_sized, new_sized, published};

const LIMIT: i64 = 64 * MIB;
const OPERATIONS: usize = 1000;
/// Every 50th publish carries the reservation of a session the reaper already released.
const REAPED_EVERY: usize = 50;
const REPLACE_TARGETS: usize = 20;

/// The workload's ports on its own pool.
struct Rig {
    db: Db,
    ledger: Arc<PgQuotaLedger>,
    service: PgTreeService,
    maintenance: PgTreeMaintenance,
    account: Uuid,
}

#[derive(Default)]
struct Tally {
    refused: AtomicI64,
    published: AtomicI64,
    reaped_failed: AtomicI64,
}

enum Op {
    Publish {
        entry: Box<PublishEntry>,
        reaped: bool,
    },
    Delete(Uuid),
    Cascade,
}

impl Rig {
    async fn reserve(&self, bytes: i64) -> Result<(), QuotaLedgerError> {
        let (ledger, account) = (self.ledger.clone(), self.account);
        let reserved = self
            .db
            .run(Scope::Account(account), async move |tx: &mut DbTx| {
                Ok::<_, sc_db::DbError>(ledger.reserve(tx, account, bytes).await)
            })
            .await
            .expect("reserve transaction");
        reserved.map(drop)
    }

    async fn release(&self, bytes: i64) {
        let (ledger, account) = (self.ledger.clone(), self.account);
        self.db
            .run(Scope::Account(account), async move |tx: &mut DbTx| {
                ledger
                    .release_reservation(tx, account, bytes)
                    .await
                    .expect("reaper release");
                Ok::<_, sc_db::DbError>(())
            })
            .await
            .expect("release transaction");
    }

    async fn publish(
        &self,
        entry: PublishEntry,
        effects: Vec<Arc<dyn CommitEffect>>,
    ) -> EntryResult {
        match self
            .service
            .publish(actor(self.account), vec![entry], effects, START)
            .await
            .expect("publish")
        {
            PublishResult::Published(mut results) => results.remove(0),
            PublishResult::Replayed(_) => panic!("unexpected replay"),
        }
    }

    async fn run(&self, op: Op, tally: &Tally) {
        match op {
            Op::Publish {
                mut entry,
                reaped: false,
            } => {
                let size = entry.revision.size;
                match self.reserve(size).await {
                    Err(QuotaLedgerError::QuotaExceeded { .. }) => {
                        tally.refused.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(other) => panic!("reserve failed: {other:?}"),
                    Ok(()) => {
                        entry.revision.reserved_bytes = size;
                        published(&self.publish(*entry, Vec::new()).await);
                        tally.published.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
            Op::Publish {
                mut entry,
                reaped: true,
            } => {
                let size = entry.revision.size;
                if self.reserve(size).await.is_err() {
                    tally.refused.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                self.release(size).await;
                entry.revision.reserved_bytes = size;
                let effect = TestEffect::failing_entries(
                    [(
                        entry.revision.revision_id,
                        EffectError::Entry(ErrorCode::UploadSessionGone),
                    )]
                    .into(),
                );
                let result = self.publish(*entry, effects(&effect)).await;
                assert!(matches!(result, EntryResult::Failed(_)), "{result:?}");
                tally.reaped_failed.fetch_add(1, Ordering::SeqCst);
            }
            Op::Delete(node) => self
                .service
                .delete_node(self.account, node, None)
                .await
                .expect("delete"),
            Op::Cascade => {
                self.maintenance
                    .cascade_batch(START, 3)
                    .await
                    .expect("cascade batch");
            }
        }
    }
}

/// `count` files of `size` under `parent` in one publish; their nodes and revisions.
async fn seed(
    tree: &Tree,
    parent: Uuid,
    prefix: &str,
    count: usize,
    size: i64,
) -> Vec<(Uuid, Uuid)> {
    let blobs = tree.blobs(count).await;
    let entries = blobs
        .iter()
        .enumerate()
        .map(|(index, blob)| new_sized(parent, &format!("{prefix}{index}"), *blob, size))
        .collect();
    tree.publish_with(entries, Vec::new())
        .await
        .iter()
        .map(|result| {
            let (_, node, revision) = published(result);
            (node, revision)
        })
        .collect()
}

async fn folder(tree: &Tree, name: &str) -> Uuid {
    tree.service
        .create_folder(tree.account, tree.root, name)
        .await
        .expect("create folder")
        .id
}

/// 600 reserve+publish (1 KiB to 1 MiB: new files, replaces, stale bases, every 50th of a reaped session), 300
/// deletes and 100 cascade batches, interleaved.
async fn operations(tree: &Tree, deletable: &[(Uuid, Uuid)], targets: &[(Uuid, Uuid)]) -> Vec<Op> {
    let blobs = tree.blobs(OPERATIONS).await;
    let (mut publishes, mut deletes) = (0usize, 0usize);
    (0..OPERATIONS)
        .map(|index| match index % 10 {
            0..=5 => {
                let k = publishes;
                publishes += 1;
                let size = KIB * (1 + i64::try_from(k * 37 % 1024).expect("small"));
                let (node, first) = targets[k % REPLACE_TARGETS];
                let entry = match k % 3 {
                    0 => new_sized(tree.root, &format!("n{k}"), blobs[k], size),
                    1 => edit_sized(node, None, blobs[k], size),
                    _ => edit_sized(node, Some(first), blobs[k], size),
                };
                Op::Publish {
                    entry: Box::new(entry),
                    reaped: k % REAPED_EVERY == REAPED_EVERY - 1,
                }
            }
            6..=8 => {
                deletes += 1;
                Op::Delete(deletable[deletes - 1].0)
            }
            _ => Op::Cascade,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_thousand_concurrent_operations_keep_usage_exact_and_never_above_the_quota() {
    // Arrange
    let tree = Tree::new().await;
    tree.exec(&format!(
        "UPDATE quota SET limit_bytes = {LIMIT} WHERE account_id = '{}'",
        tree.account
    ))
    .await;
    let d = folder(&tree, "D").await;
    let c = folder(&tree, "C").await;
    let deletable = seed(&tree, d, "d", 300, KIB).await;
    seed(&tree, c, "c", 300, KIB).await;
    let targets = seed(&tree, tree.root, "r", REPLACE_TARGETS, 4 * KIB).await;
    tree.service
        .delete_node(tree.account, c, None)
        .await
        .expect("delete C");
    let ops = operations(&tree, &deletable, &targets).await;
    let db = Db::connect(
        &DbConfig {
            db_pool_max: 16,
            ..tree.fixture.db.config()
        },
        DbRole::Api,
    )
    .await
    .expect("workload pool");
    let rig = Arc::new(Rig {
        ledger: ledger(),
        service: PgTreeService::new(
            db.clone(),
            Arc::new(StateGuard::default()),
            ledger(),
            tree.queue.clone(),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            sc_testdb::clock_at_start(),
        ),
        maintenance: PgTreeMaintenance::new(
            db.clone(),
            tree.queue.clone(),
            ledger(),
            Arc::new(PgFeedHorizon),
            &config(5000, 5000),
        ),
        db,
        account: tree.account,
    });
    let tally = Arc::new(Tally::default());
    let stop = Arc::new(AtomicBool::new(false));
    let admin = tree.quota_admin();
    let account = tree.account;
    let sampler = tokio::spawn({
        let stop = stop.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_millis(5));
            let mut samples = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                interval.tick().await;
                samples.push(admin.view(account).await.expect("sample").used);
            }
            samples
        }
    });
    let permits = Arc::new(Semaphore::new(12));

    // Act
    let tasks: Vec<_> = ops
        .into_iter()
        .map(|op| {
            let (rig, tally, permits) = (rig.clone(), tally.clone(), permits.clone());
            tokio::spawn(async move {
                let _permit = permits.acquire_owned().await.expect("permit");
                rig.run(op, &tally).await;
            })
        })
        .collect();
    for task in tasks {
        task.await.expect("operation task");
    }
    stop.store(true, Ordering::SeqCst);
    let samples = sampler.await.expect("sampler");

    // Assert
    let view = tree.view().await;
    let live = tree.live_bytes().await;
    let drift = QuotaAdmin::recompute(&tree.quota_admin(), tree.account, live, 0, false)
        .await
        .expect("recompute");
    assert!(
        samples.iter().all(|used| *used <= LIMIT),
        "max {:?}",
        samples.iter().max()
    );
    assert_eq!(view.used, live, "stored usage is live usage");
    assert_eq!((drift.stored, drift.computed), (live, live));
    assert_eq!(view.reserved, 0, "no reservation is left open");
    assert!(
        tally.refused.load(Ordering::SeqCst) > 0,
        "the quota was reached"
    );
    assert_eq!(
        tally.published.load(Ordering::SeqCst)
            + tally.refused.load(Ordering::SeqCst)
            + tally.reaped_failed.load(Ordering::SeqCst),
        600
    );
    assert!(
        tally.reaped_failed.load(Ordering::SeqCst) > 0,
        "reaped entries ran"
    );
    drop(rig);
    tree.finish().await;
}
