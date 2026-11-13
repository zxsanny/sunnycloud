//! The cascade and purge (AZ-3714) wired to the real 04 `GcQueue` (from the erase fixture) and the real 05
//! `PgFeedHorizon` and `PgFeedReader` on the test database. Rows are seeded in bulk by the superuser; deletes go
//! through the service, so every tombstone carries its own `seq`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use contracts::{Due, GcQueue, GcQueueError, GcReason, TreeMaintenance, async_trait};
use sc_clock::Timestamp;
use sc_db::{Db, DbTx};
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{FeedConfig, PgFeedHorizon, PgFeedReader};
use sunnycloud_tree::{PgTreeMaintenance, TreeConfig};
use uuid::Uuid;

use crate::lifecycle_support::Erase;
use crate::mutation_support::{DEFAULT_QUOTA, Tree, close};
use crate::support::START;

/// The real 07a ledger the cascade releases usage through.
pub fn ledger() -> Arc<PgQuotaLedger> {
    Arc::new(PgQuotaLedger::new(QuotaConfig {
        default_quota_bytes: DEFAULT_QUOTA,
    }))
}

/// Tombstones written by the service stand at `START`; this is past a 2 s horizon.
pub const PURGE_AT: Timestamp = Timestamp::from_unix_micros(START.as_unix_micros() + 3_000_000);

pub fn config(cascade_batch_size: u64, purge_batch_size: u64) -> TreeConfig {
    TreeConfig {
        tombstone_horizon: Duration::from_secs(2),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size,
        purge_batch_size,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    }
}

pub struct Maint {
    pub erase: Erase,
    pub config: TreeConfig,
    pub maintenance: Arc<PgTreeMaintenance>,
}

impl Maint {
    pub async fn new(config: TreeConfig) -> Maint {
        let erase = Erase::new().await;
        let maintenance = Arc::new(PgTreeMaintenance::new(
            erase.tree.fixture.api.clone(),
            erase.queue.clone(),
            ledger(),
            Arc::new(PgFeedHorizon),
            &config,
        ));
        Maint {
            erase,
            config,
            maintenance,
        }
    }

    pub fn tree(&self) -> &Tree {
        &self.erase.tree
    }

    /// Another process's maintenance: its own pool and the given queue.
    pub fn maintenance_on(&self, db: Db, queue: Arc<dyn GcQueue>) -> PgTreeMaintenance {
        PgTreeMaintenance::new(db, queue, ledger(), Arc::new(PgFeedHorizon), &self.config)
    }

    /// Cascade batches of `limit` until one marks nothing; the returns in order (at most 50 calls).
    pub async fn cascade_until_idle(&self, now: Timestamp, limit: u32) -> Vec<u64> {
        let mut returns = Vec::new();
        while returns.last() != Some(&0) && returns.len() < 50 {
            let marked = self
                .maintenance
                .cascade_batch(now, limit)
                .await
                .expect("cascade batch");
            returns.push(marked);
        }
        returns
    }

    /// Purge batches of `limit` until one purges nothing; the returns in order (at most 50 calls).
    pub async fn purge_until_idle(
        &self,
        maintenance: &dyn TreeMaintenance,
        limit: u32,
    ) -> Vec<u64> {
        let mut returns = Vec::new();
        while returns.last() != Some(&0) && returns.len() < 50 {
            let purged = maintenance
                .purge_tombstones(PURGE_AT, limit)
                .await
                .expect("purge batch");
            returns.push(purged);
        }
        returns
    }

    /// `count` children of `parent` of `kind`, named `<prefix><n>`, inserted in one statement; files get no revision.
    pub async fn children(
        &self,
        account: Uuid,
        parent: Uuid,
        kind: &str,
        prefix: &str,
        count: usize,
    ) -> Vec<Uuid> {
        let ids: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
        let mut admin = self.tree().fixture.db.admin().await;
        sqlx::query(
            "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
             SELECT id, $2, $3, $4, $5 || ord, $5 || ord, 1, 'epoch'::timestamptz, 'epoch'::timestamptz \
             FROM unnest($1::uuid[]) WITH ORDINALITY AS t(id, ord)",
        )
        .bind(&ids)
        .bind(account)
        .bind(parent)
        .bind(kind)
        .bind(prefix)
        .execute(&mut admin)
        .await
        .expect("insert children");
        close(admin).await;
        ids
    }

    /// `count` files of `parent`, each with one current revision of 1 byte over its own committed blob, counted
    /// in usage as `publish` would; returns `(files, blobs)`.
    pub async fn files_with_blobs(
        &self,
        account: Uuid,
        parent: Uuid,
        count: usize,
    ) -> (Vec<Uuid>, Vec<Uuid>) {
        let files = self.children(account, parent, "file", "f", count).await;
        let blobs = self.tree().blobs_of(account, count, "committed").await;
        let revisions: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
        let mut admin = self.tree().fixture.db.admin().await;
        sqlx::query(
            "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
             SELECT r, $4, n, b, 1, decode(repeat('07', 32), 'hex'), ''::bytea, 'epoch'::timestamptz \
             FROM unnest($1::uuid[], $2::uuid[], $3::uuid[]) AS t(r, n, b)",
        )
        .bind(&revisions)
        .bind(&files)
        .bind(&blobs)
        .bind(account)
        .execute(&mut admin)
        .await
        .expect("insert revisions");
        sqlx::query(
            "UPDATE nodes n SET current_revision_id = t.r FROM unnest($1::uuid[], $2::uuid[]) AS t(n, r) \
             WHERE n.id = t.n",
        )
        .bind(&files)
        .bind(&revisions)
        .execute(&mut admin)
        .await
        .expect("point files at their revisions");
        sqlx::query("UPDATE quota SET used_bytes = used_bytes + $2 WHERE account_id = $1")
            .bind(account)
            .bind(i64::try_from(count).expect("a small count"))
            .execute(&mut admin)
            .await
            .expect("count the seeded files in usage");
        close(admin).await;
        (files, blobs)
    }

    /// Deletes each node through the service, one `seq` each.
    pub async fn delete_each(&self, account: Uuid, nodes: &[Uuid]) {
        for &node in nodes {
            self.tree()
                .service
                .delete_node(account, node, None)
                .await
                .expect("delete node");
        }
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.tree().count(sql).await
    }

    pub async fn purge_horizon(&self, account: Uuid) -> i64 {
        self.count(&format!(
            "SELECT purge_horizon FROM account_feeds WHERE account_id = '{account}'"
        ))
        .await
    }

    pub fn reader(&self) -> PgFeedReader {
        let config = FeedConfig {
            longpoll_wait_max: Duration::from_secs(30),
            longpoll_auth_recheck: Duration::from_secs(2),
            feed_safety_recheck: Duration::from_secs(10),
            feed_page_limit_default: 100,
            feed_page_limit_max: None,
            max_holds_per_principal: 4,
        };
        PgFeedReader::new(self.tree().fixture.api.clone(), &config)
    }

    pub async fn finish(self) {
        let Maint {
            erase, maintenance, ..
        } = self;
        drop(maintenance);
        erase.finish().await;
    }
}

/// The real queue that, at its `kill_at`-th write, terminates its own connection mid-transaction: the process
/// running the purge dies with a half-done batch.
pub struct DyingQueue {
    pub inner: Arc<dyn GcQueue>,
    pub kill_at: u32,
    pub writes: AtomicU32,
}

#[async_trait]
impl GcQueue for DyingQueue {
    async fn schedule_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
        reason: GcReason,
        due: Due,
    ) -> Result<(), GcQueueError> {
        if self.writes.fetch_add(1, Ordering::SeqCst) + 1 == self.kill_at {
            let killed = sqlx::query("SELECT pg_terminate_backend(pg_backend_pid())")
                .execute(tx.conn())
                .await;
            let error = killed.err().map_or_else(
                || GcQueueError::Unexpected("the connection survived".into()),
                |error| GcQueueError::Unexpected(Box::new(error)),
            );
            return Err(error);
        }
        self.inner.schedule_in_tx(tx, blob_id, reason, due).await
    }

    async fn pending_for_account(&self, account: Uuid) -> Result<u64, GcQueueError> {
        self.inner.pending_for_account(account).await
    }
}
