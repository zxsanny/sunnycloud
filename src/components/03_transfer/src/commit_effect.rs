//! `BlobCommitEffect` (`commit_service.md`): run by 02's `publish` inside the entry's savepoint, after 02 wrote
//! the revision and released the entry's reservation. It locks the open session row (`FOR UPDATE`); no open row
//! (aborted, reaped or committed by another call) fails the entry with `upload_session_gone`, which rolls back
//! the entry, its release included, so the reservation is settled exactly once by whoever removed the row. With
//! the row locked the blob is marked committed and the row becomes the committed-key record. When 02 answers
//! `NoChange` (the content already is the file's current revision) the entry carries that revision instead of the
//! planned one: the upload's blob is then unused, so the row and the blob row go, and the caller discards the
//! file after COMMIT. The effect does no I/O and reads no clock.

use std::sync::Arc;

use contracts::{
    Begin, BlobRegistry, CommitEffect, EffectError, EntryOutcome, PublishedEntry, async_trait,
};
use sc_clock::Timestamp;
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_observability::log_error;
use uuid::Uuid;

use crate::{session_repo, vault_limit};

/// The one large upload a `commit_upload` call publishes.
pub(crate) struct BlobCommitEffect {
    pub registry: Arc<dyn BlobRegistry>,
    pub account: Uuid,
    pub upload_id: Uuid,
    pub blob_id: Uuid,
    pub size: i64,
    /// The revision id 03 put into `NewRevision`.
    pub revision_id: Uuid,
    /// Commit time + `upload_session_ttl`: how long the committed key stays findable.
    pub record_expires_at: Timestamp,
    pub max_vault_file_bytes: u64,
}

/// The entry failure that `commit_upload` answers as `UploadNotFound`.
pub(crate) const SESSION_GONE: ErrorCode = ErrorCode::UploadSessionGone;

impl BlobCommitEffect {
    async fn settle(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), Failure> {
        if !session_repo::lock_open(tx, self.account, self.upload_id).await? {
            return Err(Failure::Gone);
        }
        if entry.revision_id == self.revision_id {
            self.registry
                .mark_committed_in_tx(tx, self.account, self.blob_id, self.size)
                .await
                .map_err(|error| Failure::Fault(Box::new(error)))?;
            session_repo::mark_committed(
                tx,
                self.account,
                self.upload_id,
                self.revision_id,
                self.record_expires_at,
            )
            .await?;
        } else {
            // `NoChange`: the session row references the blob row, so it goes first.
            session_repo::delete_open(tx, self.account, self.upload_id).await?;
            self.registry
                .remove_row_in_tx(tx, self.account, self.blob_id)
                .await
                .map_err(|error| Failure::Fault(Box::new(error)))?;
        }
        Ok(())
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
impl CommitEffect for BlobCommitEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    async fn on_entry(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        vault_limit::check_entry(entry, self.max_vault_file_bytes)?;
        match self.settle(tx, entry).await {
            Ok(()) => Ok(()),
            Err(Failure::Gone) => Err(EffectError::Entry(SESSION_GONE)),
            Err(Failure::Fault(fault)) => {
                log_error!(
                    "commit effect failed",
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
