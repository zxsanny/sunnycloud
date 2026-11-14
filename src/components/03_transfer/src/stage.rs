//! `stage_small` (`upload_service.md`): a whole file of 0 to 2 MiB in one idempotent request, in this order:
//!
//! 1. one short transaction: the account guard, then the row `(account, staged_key)` names: an identical staged
//!    row answers `Staged` with nothing read or written, any other row is `ClientKeyMismatch`, an expired row is
//!    reaped (`crate::reaper`) and the key staged anew; then the staged-row cap (`StagedCapReached`);
//! 2. the account in-flight cap (`Overloaded`, shared with `put_block`) and the free room of the quota
//!    (`QuotaAdmin::view`), before any body byte is read;
//! 3. the body is read in full and checked for length and BLAKE3 before any write;
//! 4. `BlobWriter::write_complete` writes and syncs the file; 04 checks the free-space floor before it creates
//!    the file (`FreeSpaceFloor` writes nothing);
//! 5. one short transaction: the account guard, `QuotaLedger::reserve` (the authoritative check; its row lock
//!    serialises the account's stages, so the staged cap is checked again exactly), the `blobs` row `uploading`
//!    and a completed `upload_sessions` row (`block_count` 0 or 1, its block received). When it refuses or fails
//!    the file is discarded.
//!
//! The staged row's `client_key` is the staged key's hyphenated text (`data_model.md` 6.3).

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{
    ByteStream, RegisterState, Staged, StreamError, UploadError, WrittenBlob, WrittenOrPlanned,
};
use futures_core::Stream;
use sc_db::{RunError, Scope};
use sc_ids::{BLOCK_SIZE, Hash32, hash_block, new_id};
use sc_observability::{log_error, log_warn};
use thiserror::Error;
use uuid::Uuid;

use crate::blocks::{block_count, need_blocks};
use crate::put_block::read_block;
use crate::session_repo::{self, NewSession, StagedKeyRow};
use crate::upload::{
    PgUploadService, TxOutcome, blob_error, key_taken, quota_error, run_error, stop, unexpected,
};

/// The largest staged body: one block. A longer declared length is `LengthMismatch` (the contract names no
/// other error for it; proposal recorded in AZ-3737).
const MAX_STAGED_LEN: u64 = BLOCK_SIZE as u64;

#[derive(Debug, Error)]
enum StageFault {
    #[error("staged row expiry overflows the timestamp range")]
    ExpiryOverflow,
}

impl PgUploadService {
    pub async fn stage_small(
        &self,
        account: Uuid,
        staged_key: Uuid,
        expected_hash: Option<Hash32>,
        body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError> {
        if length > MAX_STAGED_LEN {
            return Err(UploadError::LengthMismatch);
        }
        let size = i64::try_from(length).map_err(|_| UploadError::LengthMismatch)?;
        let staged = Staged { staged_key, size };
        let key = staged_key.to_string();
        let claimed = claimed_hash(size, expected_hash.as_ref());
        if self.precheck(account, &key, size, &claimed).await? {
            return Ok(staged);
        }
        let permit = self
            .caps
            .admit(account, length)
            .ok_or(UploadError::Overloaded)?;
        self.check_room(account, size).await?;
        let bytes = read_block(body, length).await?;
        let hash = verified(&bytes, expected_hash)?;
        let written = self
            .writer
            .write_complete(account, size, one_chunk(bytes))
            .await
            .map_err(|error| self.refused(account, blob_error(error)))?;
        drop(permit);
        let blob_id = written.plan.blob_id;
        let Err(outcome) = self.record(account, &key, &written, hash.as_ref()).await else {
            return Ok(staged);
        };
        self.discard_staged(account, blob_id).await;
        match outcome {
            TxOutcome::Refused(error) => Err(self.refused(account, error)),
            // A concurrent stage with the same key committed its row first: answer by that row.
            TxOutcome::KeyTaken => match self.read_staged_key(account, &key).await? {
                Some(row) => same_row(&row, size, &claimed).map(|()| staged),
                // That row was committed and removed meanwhile; the client retries.
                None => Err(UploadError::Overloaded),
            },
        }
    }

    /// Steps 1 and the staged cap: `true` when an identical staged row already answers the request. An expired
    /// row under the key answers nothing: it is reaped here (its reservation released) and the key staged anew.
    async fn precheck(
        &self,
        account: Uuid,
        key: &str,
        size: i64,
        claimed: &[u8],
    ) -> Result<bool, UploadError> {
        let now = self.clock.now_utc();
        let (answered, reaped) = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await?;
                let row = session_repo::staged_by_key(tx, account, key)
                    .await
                    .map_err(unexpected)?;
                let mut reaped = Vec::new();
                if let Some(row) = row {
                    if !session_repo::is_expired(row.expires_at, now) {
                        return same_row(&row, size, claimed).map(|()| (true, reaped));
                    }
                    reaped = self
                        .reap_in_tx(tx, account, Some(key), now, 1)
                        .await
                        .map_err(UploadError::Unexpected)?;
                }
                let staged = session_repo::count_staged(tx, account)
                    .await
                    .map_err(unexpected)?;
                self.below_staged_cap(staged)?;
                Ok((false, reaped))
            })
            .await
            .map_err(run_error)?;
        self.reaped(account, &reaped).await;
        Ok(answered)
    }

    async fn read_staged_key(
        &self,
        account: Uuid,
        key: &str,
    ) -> Result<Option<StagedKeyRow>, UploadError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                session_repo::staged_by_key(tx, account, key)
                    .await
                    .map_err(unexpected)
            })
            .await
            .map_err(run_error)
    }

    fn below_staged_cap(&self, staged: i64) -> Result<(), UploadError> {
        if u64::try_from(staged).unwrap_or(u64::MAX) >= self.config.max_staged {
            return Err(UploadError::StagedCapReached);
        }
        Ok(())
    }

    /// The pre-check of the quota; `reserve` in the row transaction stays the authoritative one.
    async fn check_room(&self, account: Uuid, size: i64) -> Result<(), UploadError> {
        let view = self
            .quota_admin
            .view(account)
            .await
            .map_err(|error| UploadError::Unexpected(Box::new(error)))?;
        let room = view
            .effective_limit
            .saturating_sub(view.used)
            .saturating_sub(view.reserved);
        if room < size {
            return Err(self.refused(account, UploadError::QuotaExceeded));
        }
        Ok(())
    }

    /// The row transaction of step 5.
    async fn record(
        &self,
        account: Uuid,
        key: &str,
        written: &WrittenBlob,
        hash: Option<&Hash32>,
    ) -> Result<(), TxOutcome> {
        let now = self.clock.now_utc();
        let expires_at = now
            .checked_add(self.config.upload_session_ttl)
            .ok_or_else(|| {
                stop(UploadError::Unexpected(Box::new(
                    StageFault::ExpiryOverflow,
                )))
            })?;
        let block_count = block_count(written.size).map_err(stop)?;
        let session = NewSession {
            id: new_id(self.clock.as_ref()),
            account,
            blob_id: written.plan.blob_id,
            client_key: key,
            declared_size: written.size,
            block_count,
            now,
            expires_at,
        };
        let result = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await.map_err(stop)?;
                self.quota
                    .reserve(tx, account, written.size)
                    .await
                    .map_err(|error| stop(quota_error(error)))?;
                let staged = session_repo::count_staged(tx, account)
                    .await
                    .map_err(|error| stop(unexpected(error)))?;
                self.below_staged_cap(staged).map_err(stop)?;
                self.registry
                    .register_in_tx(
                        tx,
                        account,
                        WrittenOrPlanned::Written(written.clone()),
                        RegisterState::Uploading,
                    )
                    .await
                    .map_err(|error| stop(UploadError::Unexpected(Box::new(error))))?;
                session_repo::insert_staged(tx, &session, hash)
                    .await
                    .map_err(key_taken)
            })
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(RunError::Closure(outcome)) => Err(outcome),
            Err(RunError::Db(error)) => Err(stop(unexpected(error))),
        }
    }

    /// Best effort: a file left behind has no row and is 04's orphan sweeper's.
    async fn discard_staged(&self, account: Uuid, blob_id: Uuid) {
        if let Err(error) = self.writer.discard(blob_id).await {
            log_error!(
                "staged file not discarded",
                account_id = account,
                err = &error
            );
        }
    }
}

/// The hash a request claims, as a staged row stores its first block hash: empty for a zero-byte file (with no
/// hash, or the hash of no bytes), the 32 bytes otherwise (empty when bytes come without a hash).
fn claimed_hash(size: i64, expected: Option<&Hash32>) -> Vec<u8> {
    match expected {
        Some(hash) if size > 0 || *hash != hash_block(&[]) => hash.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

/// An existing row answers `Staged` only when it is an uncommitted staged row of the same size and hash with its
/// block received; anything else under the key is `ClientKeyMismatch`.
fn same_row(row: &StagedKeyRow, size: i64, claimed: &[u8]) -> Result<(), UploadError> {
    let same = !row.committed
        && row.declared_size == size
        && block_count(size).is_ok_and(|count| count == row.block_count)
        && need_blocks(row.block_count, &row.received).is_empty()
        && row.first_hash == claimed;
    if same {
        Ok(())
    } else {
        Err(UploadError::ClientKeyMismatch)
    }
}

/// The body's block hash, `None` for an empty body. A zero-byte file may omit the hash; bytes without a hash, or
/// with another one, are `HashMismatch`.
fn verified(bytes: &[u8], expected: Option<Hash32>) -> Result<Option<Hash32>, UploadError> {
    let actual = hash_block(bytes);
    match expected {
        None if bytes.is_empty() => Ok(None),
        Some(hash) if hash == actual => Ok((!bytes.is_empty()).then_some(actual)),
        _ => {
            log_warn!("staged body hash mismatch");
            Err(UploadError::HashMismatch)
        }
    }
}

/// The verified body handed to the writer as one piece.
struct OneChunk(Option<Bytes>);

impl Stream for OneChunk {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

fn one_chunk(bytes: Vec<u8>) -> ByteStream {
    Box::pin(OneChunk(Some(Bytes::from(bytes))))
}
