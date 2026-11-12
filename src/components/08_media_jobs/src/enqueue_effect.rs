//! `MediaEnqueueEffect`: 08's `CommitEffect`, run by 02 inside the commit transaction of one account
//! (`media_enqueue_effect.md`). Per entry it issues at most one probe on `derived`, one count on the queued-jobs
//! index and one insert per planned job, all on the commit transaction, so the jobs exist only if the entry's
//! savepoint and the commit do.

use std::sync::Arc;

use contracts::{Begin, CommitEffect, EffectError, EntryOutcome, PublishedEntry, async_trait};
use media_shared::policy::JobSpec;
use sc_db::{DbError, DbTx};
use sc_error::ErrorCode;
use sc_observability::log_error;
use uuid::Uuid;

use crate::job_queue::{JobQueue, Priority};
use crate::planner::JobPlanner;
use crate::{derived_repo, job_repo};

/// Built per `publish` call for the committing account (`CommitActor::account_id`; `PublishedEntry` carries none).
pub struct MediaEnqueueEffect {
    queue: Arc<JobQueue>,
    planner: JobPlanner,
    account_id: Uuid,
}

impl MediaEnqueueEffect {
    pub fn new(queue: Arc<JobQueue>, planner: JobPlanner, account_id: Uuid) -> Self {
        MediaEnqueueEffect {
            queue,
            planner,
            account_id,
        }
    }

    async fn enqueue(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), DbError> {
        let planned = self.planner.plan(self.account_id, entry);
        if planned.is_empty() {
            return Ok(());
        }
        let present = derived_repo::kinds_present(
            tx,
            self.account_id,
            &entry.content_hash,
            self.planner.params_version(),
        )
        .await?;
        let due: Vec<JobSpec> = planned
            .into_iter()
            .filter(|spec| {
                !spec
                    .kind
                    .outputs()
                    .iter()
                    .all(|kind| present.contains(kind))
            })
            .collect();
        if due.is_empty() {
            return Ok(());
        }
        let priority = self.priority(tx, entry.import_hint).await?;
        for spec in due {
            self.queue.enqueue_in_tx(tx, spec, priority).await?;
        }
        Ok(())
    }

    /// The hint alone makes the entry bulk, so the count is probed only without it.
    async fn priority(&self, tx: &mut DbTx, import_hint: bool) -> Result<Priority, DbError> {
        if import_hint {
            return Ok(self.planner.priority(true, 0));
        }
        let queued =
            job_repo::count_queued(tx, self.account_id, self.planner.count_limit()).await?;
        Ok(self.planner.priority(false, queued))
    }
}

#[async_trait]
impl CommitEffect for MediaEnqueueEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    /// A database failure is unexpected (tier 3): it is logged and rolls back the whole commit.
    async fn on_entry(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        self.enqueue(tx, entry).await.map_err(|error| {
            log_error!(
                "media job enqueue failed",
                account_id = self.account_id,
                err = &error
            );
            EffectError::Batch(ErrorCode::Internal)
        })
    }

    async fn on_batch(
        &self,
        _tx: &mut DbTx,
        _outcomes: &[EntryOutcome],
    ) -> Result<(), EffectError> {
        Ok(())
    }
}
