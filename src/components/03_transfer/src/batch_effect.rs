//! The effects `commit_batch` hands to 02's `publish` (`commit_service.md`). Both are re-runnable: `publish`
//! may retry its whole transaction, so they keep no state between calls. They do no I/O and read no clock.
//!
//! `ReceiptEffect` looks the commit id up in `on_begin`, after `publish` took the account lock, so two identical
//! batches can never both miss the receipt (no unique violation is relied on): the same request hash replays the
//! stored response, another one is `CommitIdReused`. `on_batch` stores every outcome, entries refused before
//! `publish` included.
//!
//! `StagedCommitEffect` is `BlobCommitEffect` for staged rows: it locks the entry's staged row; no open row
//! (committed by an earlier entry or call, or reaped) fails the entry with `upload_session_gone`, which rolls back
//! the entry and its reservation release. With the row locked the blob is marked committed and the row deleted.
//! The entry is found by the planned revision id; a `NoChange` entry carries the file's existing revision
//! instead, so it is found by its node and content hash (only an `Existing` target can answer `NoChange`), the
//! first such row still open in entry order; its blob is unused, so the row and the blob row go and the caller
//! discards the file after COMMIT.

use std::collections::HashMap;
use std::sync::Arc;

use contracts::{
    Begin, BlobRegistry, CommitEffect, EffectError, EntryError, EntryOutcome, PublishedEntry,
    StoredResponse, async_trait,
};
use sc_clock::Timestamp;
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_ids::{FileHash, Hash32};
use sc_observability::log_error;
use uuid::Uuid;

use crate::batch_response::{assemble, encode};
use crate::commit_effect::SESSION_GONE;
use crate::{receipt_repo, session_repo};

pub(crate) struct ReceiptEffect {
    pub account: Uuid,
    pub commit_id: Uuid,
    pub request_hash: Hash32,
    /// Per request entry: `Some` when refused before `publish`, `None` for the next entry handed to it.
    pub refused: Vec<Option<EntryError>>,
    pub now: Timestamp,
}

#[async_trait]
impl CommitEffect for ReceiptEffect {
    async fn on_begin(&self, tx: &mut DbTx) -> Result<Begin, EffectError> {
        let found = receipt_repo::find(tx, self.account, self.commit_id)
            .await
            .map_err(|error| batch_fault(self.account, &error))?;
        match found {
            None => Ok(Begin::Proceed),
            Some(receipt) if receipt.request_hash == self.request_hash.as_bytes() => {
                Ok(Begin::Replay(StoredResponse(receipt.response.into_bytes())))
            }
            Some(_) => Err(EffectError::Batch(ErrorCode::CommitIdReused)),
        }
    }

    async fn on_entry(&self, _tx: &mut DbTx, _entry: &PublishedEntry) -> Result<(), EffectError> {
        Ok(())
    }

    async fn on_batch(&self, tx: &mut DbTx, outcomes: &[EntryOutcome]) -> Result<(), EffectError> {
        let results = outcomes.iter().map(|outcome| &outcome.result);
        let response =
            assemble(&self.refused, results).map_err(|error| batch_fault(self.account, &error))?;
        receipt_repo::insert(
            tx,
            self.account,
            self.commit_id,
            &self.request_hash,
            &encode(&response),
            self.now,
        )
        .await
        .map_err(|error| batch_fault(self.account, &error))
    }
}

/// A receipt read, response or write that failed: the whole call fails, nothing is applied.
fn batch_fault(account: Uuid, error: &(dyn std::error::Error + 'static)) -> EffectError {
    log_error!("commit receipt failed", account_id = account, err = error);
    EffectError::Batch(ErrorCode::Internal)
}

/// One entry handed to `publish`: its staged row, the planned revision and, for an `Existing` target, the node.
pub(crate) struct StagedPlan {
    pub session_id: Uuid,
    pub blob_id: Uuid,
    pub size: i64,
    pub revision_id: Uuid,
    pub existing_node: Option<Uuid>,
    pub content_hash: FileHash,
}

pub(crate) struct StagedCommitEffect {
    pub registry: Arc<dyn BlobRegistry>,
    pub account: Uuid,
    pub plans: Vec<StagedPlan>,
    /// Planned revision id -> index into `plans`.
    pub by_revision: HashMap<Uuid, usize>,
}

impl StagedCommitEffect {
    pub(crate) fn new(
        registry: Arc<dyn BlobRegistry>,
        account: Uuid,
        plans: Vec<StagedPlan>,
    ) -> Self {
        let by_revision = plans
            .iter()
            .enumerate()
            .map(|(index, plan)| (plan.revision_id, index))
            .collect();
        StagedCommitEffect {
            registry,
            account,
            plans,
            by_revision,
        }
    }

    async fn settle(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), Failure> {
        if let Some(&index) = self.by_revision.get(&entry.revision_id) {
            let plan = &self.plans[index];
            if !session_repo::lock_open(tx, self.account, plan.session_id).await? {
                return Err(Failure::Gone);
            }
            self.registry
                .mark_committed_in_tx(tx, self.account, plan.blob_id, plan.size)
                .await
                .map_err(|error| Failure::Fault(Box::new(error)))?;
            session_repo::delete_open(tx, self.account, plan.session_id).await?;
            return Ok(());
        }
        let candidates = self.plans.iter().filter(|plan| {
            plan.existing_node == Some(entry.node_id) && plan.content_hash == entry.content_hash
        });
        for plan in candidates {
            if session_repo::lock_open(tx, self.account, plan.session_id).await? {
                // `NoChange`: the staged row references the blob row, so it goes first.
                session_repo::delete_open(tx, self.account, plan.session_id).await?;
                self.registry
                    .remove_row_in_tx(tx, self.account, plan.blob_id)
                    .await
                    .map_err(|error| Failure::Fault(Box::new(error)))?;
                return Ok(());
            }
        }
        Err(Failure::Gone)
    }
}

enum Failure {
    Gone,
    Fault(contracts::Fault),
}

impl From<sc_db::DbError> for Failure {
    fn from(error: sc_db::DbError) -> Self {
        Failure::Fault(Box::new(error))
    }
}

#[async_trait]
impl CommitEffect for StagedCommitEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    async fn on_entry(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        match self.settle(tx, entry).await {
            Ok(()) => Ok(()),
            Err(Failure::Gone) => Err(EffectError::Entry(SESSION_GONE)),
            Err(Failure::Fault(fault)) => {
                log_error!(
                    "staged commit effect failed",
                    account_id = self.account,
                    err = fault.as_ref()
                );
                Err(EffectError::Entry(ErrorCode::Internal))
            }
        }
    }

    async fn on_batch(
        &self,
        _tx: &mut DbTx,
        _outcomes: &[EntryOutcome],
    ) -> Result<(), EffectError> {
        Ok(())
    }
}
