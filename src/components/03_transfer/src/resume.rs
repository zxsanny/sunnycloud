//! `get_upload` and `abort_upload` (`upload_service.md`), and the lazy load of a session's progress entry.
//!
//! Progress is never loaded at start. The first access to a session this process has no entry for (after a
//! restart) reads its open, unexpired row in one short transaction, asks 04 for the uploading blob's plan, reopens
//! the file for writing and builds the entry from `received` and `block_hashes`: blocks acknowledged after the
//! last checkpoint are needed again.
//!
//! `abort_upload` is one short transaction: the account guard, `DELETE … RETURNING blob_id, reserved_bytes`,
//! `QuotaLedger::release_reservation` for exactly that amount and `BlobRegistry::remove_row_in_tx`. Whoever
//! deletes the row settles the reservation, so a repeat (or an abort racing the reaper or a commit) finds no row,
//! answers `UploadNotFound` and releases nothing. The file is discarded and the entry evicted after COMMIT.

use std::sync::Arc;

use contracts::{UploadError, UploadState};
use sc_db::Scope;
use sc_observability::log_error;
use thiserror::Error;
use uuid::Uuid;

use crate::blocks::as_u32;
use crate::checkpoint::CheckpointMark;
use crate::progress::{Checkpoint, SessionInfo, SharedProgress};
use crate::session_repo::{self, OpenRow};
use crate::upload::{PgUploadService, blob_error, run_error, unexpected};
use crate::upload_metrics::SessionResult;

/// An unexpected failure of a lazy load: the rows of 03 and 04 disagree.
#[derive(Debug, Error)]
enum LoadFault {
    #[error("the blob of an open upload session is not uploading")]
    BlobNotUploading,
}

impl PgUploadService {
    pub async fn get_upload(
        &self,
        account: Uuid,
        upload_id: Uuid,
    ) -> Result<UploadState, UploadError> {
        let session = self.live_session(account, upload_id).await?;
        // The checkpoint lock is never taken while the progress lock is held.
        let expires_at = session.expires_at();
        let progress = session.lock();
        Ok(UploadState {
            need_blocks: progress.missing_blocks(),
            received_count: progress.received_count(),
            expires_at,
        })
    }

    pub async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError> {
        let now = self.clock.now_utc();
        let blob_id = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await?;
                let (blob_id, reserved) =
                    session_repo::delete_open_returning(tx, account, upload_id, now)
                        .await
                        .map_err(unexpected)?
                        .ok_or(UploadError::UploadNotFound)?;
                self.quota
                    .release_reservation(tx, account, reserved)
                    .await
                    .map_err(|error| UploadError::Unexpected(Box::new(error)))?;
                self.registry
                    .remove_row_in_tx(tx, account, blob_id)
                    .await
                    .map_err(|error| UploadError::Unexpected(Box::new(error)))?;
                Ok(blob_id)
            })
            .await
            .map_err(run_error)?;
        self.evict(upload_id);
        // Best effort after COMMIT: a file left behind has no row and is the orphan sweeper's (04).
        if let Err(error) = self.writer.discard(blob_id).await {
            log_error!(
                "aborted upload file not discarded",
                account_id = account,
                err = &error
            );
        }
        if let Some(metrics) = &self.metrics {
            metrics.session_ended(SessionResult::Aborted);
        }
        Ok(())
    }

    /// Drops the session's entry; `upload_sessions_active` falls only when an entry was there.
    pub(crate) fn evict(&self, upload_id: Uuid) {
        let evicted = self.progress.remove(upload_id);
        if let Some(metrics) = self.metrics.as_ref().filter(|_| evicted) {
            metrics.session_closed();
        }
    }

    /// The session's entry when it belongs to `account` and has not expired, loaded from its checkpoint row when
    /// this process has none. An expired entry is dropped.
    pub(crate) async fn live_session(
        &self,
        account: Uuid,
        upload_id: Uuid,
    ) -> Result<Arc<SharedProgress>, UploadError> {
        let session = match self.progress.get(upload_id) {
            Some(found) if found.info().account == account => found,
            Some(_) => return Err(UploadError::UploadNotFound),
            None => self.load(account, upload_id).await?,
        };
        if self.clock.now_utc() >= session.expires_at() {
            self.evict(upload_id);
            return Err(UploadError::UploadNotFound);
        }
        Ok(session)
    }

    /// Builds the entry from the open, unexpired row and reopens the blob file for writing. An entry another
    /// request loaded meanwhile wins.
    async fn load(
        &self,
        account: Uuid,
        upload_id: Uuid,
    ) -> Result<Arc<SharedProgress>, UploadError> {
        let now = self.clock.now_utc();
        let row = self
            .db
            .run(Scope::Account(account), async |tx| {
                session_repo::open_session(tx, account, upload_id, now)
                    .await
                    .map_err(unexpected)
            })
            .await
            .map_err(run_error)?
            .ok_or(UploadError::UploadNotFound)?;
        let plan = self
            .catalog
            .get_uploading_plan(account, row.blob_id)
            .await
            .map_err(|error| UploadError::Unexpected(Box::new(error)))?
            .ok_or_else(|| UploadError::Unexpected(Box::new(LoadFault::BlobNotUploading)))?;
        let handle = self
            .writer
            .open_for_write(
                plan.blob_id,
                &plan.wrapped_key,
                plan.master_key_id,
                row.declared_size,
            )
            .await
            .map_err(blob_error)?;
        let info = SessionInfo {
            account,
            declared_size: row.declared_size,
            handle,
        };
        let mark = CheckpointMark {
            at: self.clock.monotonic(),
            expires_at: row.expires_at,
        };
        let (loaded, created) = self
            .progress
            .load(upload_id, info, mark, &checkpoint_of(&row))
            .map_err(|error| UploadError::Unexpected(Box::new(error)))?;
        if let Some(metrics) = self.metrics.as_ref().filter(|_| created) {
            metrics.session_opened();
        }
        Ok(loaded)
    }
}

fn checkpoint_of(row: &OpenRow) -> Checkpoint<'_> {
    Checkpoint {
        block_count: as_u32(row.block_count),
        received: &row.received,
        block_hashes: &row.block_hashes,
    }
}
