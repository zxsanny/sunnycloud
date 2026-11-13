//! The usage deltas and queue writes of the tree (AZ-3711): entries of a chosen size, one-entry publishes, and
//! the three views of usage the tests compare: the stored row (`QuotaAdmin::view` of the real 07a admin), the
//! truth (`LiveUsageSource::live_bytes` of the real reader) and the account's `blob_gc` rows (the real 04 queue).

use std::sync::Arc;

use contracts::{
    CommitEffect, EntryResult, LiveUsageSource, PublishEntry, PublishOutcome, PublishResult,
    QuotaAdmin, QuotaView, TreeCommit,
};
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use uuid::Uuid;

use crate::mutation_support::{DEFAULT_QUOTA, Tree, close};
use crate::publish_support::{actor, existing, new_file};
use crate::reader_support::reader;

pub const KIB: i64 = 1024;
pub const MIB: i64 = 1024 * KIB;

/// The retention of the test queue (`lifecycle_support::config`).
pub const RETENTION_MICROS: i64 = 7 * 86_400 * 1_000_000;

/// A new file of `size` bytes under `parent` over `blob`.
pub fn new_sized(parent: Uuid, name: &str, blob: Uuid, size: i64) -> PublishEntry {
    let mut entry = new_file(parent, name, blob);
    entry.revision.size = size;
    entry
}

/// A new revision of `size` bytes of `node` over `blob`, on `base`.
pub fn edit_sized(node: Uuid, base: Option<Uuid>, blob: Uuid, size: i64) -> PublishEntry {
    let mut entry = existing(node, base, blob);
    entry.revision.size = size;
    entry
}

/// The outcome, node and revision of a published entry.
pub fn published(result: &EntryResult) -> (PublishOutcome, Uuid, Uuid) {
    match result {
        EntryResult::Published {
            outcome,
            node_id,
            revision_id,
            ..
        } => (*outcome, *node_id, *revision_id),
        failed @ EntryResult::Failed(_) => panic!("expected a published entry, got {failed:?}"),
    }
}

impl Tree {
    /// Publishes `entries` with `effects` at the start time; the entry results in order.
    pub async fn publish_with(
        &self,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
    ) -> Vec<EntryResult> {
        match self
            .service
            .publish(actor(self.account), entries, effects, sc_testdb::START)
            .await
            .expect("publish")
        {
            PublishResult::Published(results) => results,
            PublishResult::Replayed(_) => panic!("unexpected replay"),
        }
    }

    /// Publishes one entry with no effect: its outcome, node and revision.
    pub async fn publish_one(&self, entry: PublishEntry) -> (PublishOutcome, Uuid, Uuid) {
        published(&self.publish_with(vec![entry], Vec::new()).await[0])
    }

    pub fn quota_admin(&self) -> PgQuotaAdmin {
        PgQuotaAdmin::new(
            self.fixture.api.clone(),
            QuotaConfig {
                default_quota_bytes: DEFAULT_QUOTA,
            },
        )
    }

    pub async fn view(&self) -> QuotaView {
        QuotaAdmin::view(&self.quota_admin(), self.account)
            .await
            .expect("quota view")
    }

    pub async fn live_bytes(&self) -> i64 {
        reader(self, 1000)
            .live_bytes(self.account)
            .await
            .expect("live bytes")
    }

    /// The `blob_gc` row of `blob`: its reason and `due_at` in microseconds.
    pub async fn gc_row(&self, blob: Uuid) -> Option<(String, i64)> {
        let mut admin = self.fixture.db.admin().await;
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT reason, (extract(epoch FROM due_at) * 1000000)::bigint FROM blob_gc WHERE blob_id = $1",
        )
        .bind(blob)
        .fetch_optional(&mut admin)
        .await
        .expect("read blob_gc row");
        close(admin).await;
        row
    }

    /// The account's `blob_gc` rows with `reason`.
    pub async fn queued(&self, reason: &str) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM blob_gc WHERE account_id = '{}' AND reason = '{reason}'",
            self.account
        ))
        .await
    }
}
