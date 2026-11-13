//! `TreeMaintenance`: the folder cascade and the tombstone purge. Each call is one short system-scope
//! transaction bounded by `limit`, in which the cascade also releases the usage of the files it marks and queues
//! their blobs (AZ-3711), retried once when the database aborts it for a deadlock (with a concurrent
//! `publish`, the GC sweeper or the erase hook). No loop state exists beyond `cascade_pending` and `deleted_at`,
//! so a run that dies anywhere is resumed by the next call's query.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Due, FeedHorizon, FeedHorizonError, GcQueue, GcQueueError, GcReason, MaintenanceError,
    QuotaLedger, QuotaLedgerError, Seq, TreeMaintenance, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{Db, DbError, DbTx};
use uuid::Uuid;

use crate::config::TreeConfig;
use crate::lifecycle_repo;
use crate::maintenance_repo::{self, Leaf};
use crate::retry::retry_aborted;

/// Separate from `PgTreeService` because it needs the feed horizon and none of the request path's other ports;
/// 12 wraps it in the two `TreeJanitor`s.
pub struct PgTreeMaintenance {
    db: Db,
    queue: Arc<dyn GcQueue>,
    quota: Arc<dyn QuotaLedger>,
    horizon: Arc<dyn FeedHorizon>,
    tombstone_horizon: Duration,
}

impl PgTreeMaintenance {
    pub fn new(
        db: Db,
        queue: Arc<dyn GcQueue>,
        quota: Arc<dyn QuotaLedger>,
        horizon: Arc<dyn FeedHorizon>,
        config: &TreeConfig,
    ) -> Self {
        PgTreeMaintenance {
            db,
            queue,
            quota,
            horizon,
            tombstone_horizon: config.tombstone_horizon,
        }
    }

    /// Pending folders one after another, each marked up to what is left of `limit`. A folder whose children are
    /// all marked loses its flag in the same transaction; the subfolders just marked are claimed next.
    /// `limit` also bounds the folders claimed, so a run of empty pending folders cannot grow the transaction.
    /// Each marked file's blob is queued `deleted` as it is marked; the sizes are summed per account and leave
    /// usage in one `adjust_used` per account at the end, so the quota row is touched once per transaction.
    async fn cascade_in(
        &self,
        tx: &mut DbTx,
        now: Timestamp,
        limit: u32,
    ) -> Result<u64, MaintenanceError> {
        let mut budget = u64::from(limit);
        let (mut marked, mut claims) = (0, 0);
        let mut freed: BTreeMap<Uuid, i64> = BTreeMap::new();
        while budget > 0 && claims < limit {
            let Some((folder, account)) = maintenance_repo::claim_pending(tx)
                .await
                .map_err(unexpected)?
            else {
                break;
            };
            claims += 1;
            let children = maintenance_repo::mark_children(tx, account, folder, now, budget)
                .await
                .map_err(unexpected)?;
            let done = children.len() as u64;
            for file in children.into_iter().flatten() {
                self.queue
                    .schedule_in_tx(tx, file.blob_id, GcReason::Deleted, Due::AfterRetention)
                    .await
                    .map_err(|GcQueueError::Unexpected(fault)| {
                        MaintenanceError::Unexpected(fault)
                    })?;
                let sum = freed.entry(account).or_insert(0);
                *sum = sum.saturating_add(file.size);
            }
            marked += done;
            budget = budget.saturating_sub(done);
            let cleared = maintenance_repo::clear_pending(tx, account, folder)
                .await
                .map_err(unexpected)?;
            if !cleared {
                break;
            }
        }
        for (account, bytes) in freed {
            if bytes != 0 {
                self.quota
                    .adjust_used(tx, account, -bytes)
                    .await
                    .map_err(|error| match error {
                        QuotaLedgerError::Unexpected(fault) => MaintenanceError::Unexpected(fault),
                        other => MaintenanceError::Unexpected(Box::new(other)),
                    })?;
            }
        }
        Ok(marked)
    }

    /// Leaves level by level up to `limit`, then the purge horizon of every account touched: those calls are the
    /// last statements of the transaction, so the horizon covers every node it purged.
    async fn purge_in(
        &self,
        tx: &mut DbTx,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<u64, MaintenanceError> {
        let mut budget = u64::from(limit);
        let mut highest: BTreeMap<Uuid, Seq> = BTreeMap::new();
        let mut purged = 0;
        while budget > 0 {
            let leaves = maintenance_repo::claim_leaves(tx, cutoff, budget)
                .await
                .map_err(unexpected)?;
            if leaves.is_empty() {
                break;
            }
            for (account, nodes) in by_account(&leaves) {
                purged += self.purge_nodes(tx, account, &nodes).await?;
            }
            for leaf in &leaves {
                let seq = highest.entry(leaf.account).or_insert(leaf.seq);
                *seq = (*seq).max(leaf.seq);
            }
            budget = budget.saturating_sub(leaves.len() as u64);
        }
        for (account, seq) in highest {
            self.horizon
                .raise_purge_horizon(tx, account, seq)
                .await
                .map_err(|FeedHorizonError::Unexpected(fault)| {
                    MaintenanceError::Unexpected(fault)
                })?;
        }
        Ok(purged)
    }

    /// Each blob row is locked by its queue write before its revision is deleted, the order the GC sweeper uses
    /// (blob row, then `remove_owner_rows`), then the nodes go.
    async fn purge_nodes(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        nodes: &[Uuid],
    ) -> Result<u64, MaintenanceError> {
        let owned = maintenance_repo::revisions_of(tx, account, nodes)
            .await
            .map_err(unexpected)?;
        for row in &owned {
            self.queue
                .schedule_in_tx(tx, row.blob_id, GcReason::Deleted, Due::AfterRetention)
                .await
                .map_err(|GcQueueError::Unexpected(fault)| MaintenanceError::Unexpected(fault))?;
        }
        let ids: Vec<Uuid> = owned.iter().map(|row| row.revision_id).collect();
        lifecycle_repo::delete_revisions(tx, account, &ids)
            .await
            .map_err(unexpected)?;
        maintenance_repo::delete_nodes(tx, account, nodes)
            .await
            .map_err(unexpected)
    }
}

#[async_trait]
impl TreeMaintenance for PgTreeMaintenance {
    /// Returns the descendants marked; 0 means no pending folder was left to claim, or `limit` empty folders
    /// were cleared.
    async fn cascade_batch(&self, now: Timestamp, limit: u32) -> Result<u64, MaintenanceError> {
        retry_aborted(|| {
            maintenance_repo::in_system(&self.db, "tree_cascade", async move |tx: &mut DbTx| {
                self.cascade_in(tx, now, limit).await
            })
        })
        .await
    }

    /// Returns the nodes purged, tombstoned at or before `now - tombstone_horizon`.
    async fn purge_tombstones(&self, now: Timestamp, limit: u32) -> Result<u64, MaintenanceError> {
        let Some(cutoff) = now.checked_sub(self.tombstone_horizon) else {
            return Ok(0);
        };
        retry_aborted(|| {
            maintenance_repo::in_system(&self.db, "tree_purge", async move |tx: &mut DbTx| {
                self.purge_in(tx, cutoff, limit).await
            })
        })
        .await
    }
}

/// The leaves' ids per account, accounts in a stable order.
fn by_account(leaves: &[Leaf]) -> BTreeMap<Uuid, Vec<Uuid>> {
    let mut groups: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for leaf in leaves {
        groups.entry(leaf.account).or_default().push(leaf.id);
    }
    groups
}

fn unexpected(error: DbError) -> MaintenanceError {
    MaintenanceError::Unexpected(Box::new(error))
}
