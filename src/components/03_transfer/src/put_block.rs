//! `put_block` (`upload_service.md`): one block of a large upload session, in this order and without a
//! database call on the way:
//!
//! 1. the session from the progress table (absent, foreign or past `expires_at` is `UploadNotFound`), the index
//!    and the declared length (`min(2 MiB, remaining)`);
//! 2. the duplicate rules and the session's lane cap, then the account and buffer caps: a repeat of a received
//!    block with its hash is acknowledged, another hash for a received or in-flight index is `BlockConflict`,
//!    and a busy lane, account or buffer is `Overloaded`, all before any body byte is read;
//! 3. the body is read in full and checked for length and BLAKE3 before any write;
//! 4. `write_block` and the group `fdatasync` (`await_durable`); only then the received bit and hash are set.
//!
//! Every `checkpoint_bytes` or `checkpoint_interval` one short transaction checks the account (`AccountGuard`,
//! owner decision 2026-11-13: a deleting account is refused within one checkpoint) and rewrites `received`,
//! `block_hashes` and the rolling `expires_at`. No transaction is open while the body is read or the block
//! synced.

use contracts::{BlobError, BlobHandle, BlockAck, ByteStream, StreamError, UploadError};
use sc_db::Scope;
use sc_ids::{Hash32, hash_block};
use sc_observability::{log_error, log_warn};
use thiserror::Error;
use uuid::Uuid;

use crate::blocks::expected_len;
use crate::checkpoint::{CheckpointMark, CheckpointPolicy};
use crate::progress::{BlockStart, SharedProgress};
use crate::session_repo::{self, Progress};
use crate::upload::{PgUploadService, run_error, unexpected};

/// Unexpected failures of a block: none is a client mistake.
#[derive(Debug, Error)]
enum BlockFault {
    #[error("the request body stream reported stored-content corruption")]
    Body(#[source] StreamError),
    #[error("session expiry overflows the timestamp range")]
    ExpiryOverflow,
}

impl PgUploadService {
    pub async fn put_block(
        &self,
        account: Uuid,
        upload_id: Uuid,
        index: u32,
        expected_hash: Hash32,
        body: ByteStream,
        length: u64,
    ) -> Result<BlockAck, UploadError> {
        let session = self.live_session(account, upload_id)?;
        let expected = expected_len(session.info().declared_size, index)
            .ok_or(UploadError::IndexOutOfRange)?;
        if length != expected {
            return Err(UploadError::LengthMismatch);
        }
        let lane_limit = usize::try_from(self.config.max_lanes).unwrap_or(usize::MAX);
        let start = session
            .lock()
            .start_block(index, expected_hash, lane_limit)
            .map_err(|_| UploadError::IndexOutOfRange)?;
        match start {
            BlockStart::Started => {}
            BlockStart::Received => return Ok(BlockAck),
            BlockStart::ReceivedOther | BlockStart::InFlightOther => {
                return Err(UploadError::BlockConflict);
            }
            BlockStart::InFlight | BlockStart::LanesFull => return Err(UploadError::Overloaded),
        }
        let lane = Lane {
            session: &session,
            index,
            held: true,
        };
        let permit = self
            .caps
            .admit(account, expected)
            .ok_or(UploadError::Overloaded)?;
        let block = read_block(body, expected).await?;
        if hash_block(&block) != expected_hash {
            log_warn!("block hash mismatch", account_id = account);
            return Err(UploadError::HashMismatch);
        }
        self.write_durable(account, &session.info().handle, index, block)
            .await?;
        let fresh = lane.received(expected_hash)?;
        drop(permit);
        if let Some(metrics) = self.metrics.as_ref().filter(|_| fresh) {
            metrics.block_received(expected);
        }
        self.checkpoint_if_due(&session, upload_id, expected)
            .await?;
        Ok(BlockAck)
    }

    /// The session's entry when it belongs to `account` and has not expired; an expired entry is dropped.
    fn live_session(
        &self,
        account: Uuid,
        upload_id: Uuid,
    ) -> Result<std::sync::Arc<SharedProgress>, UploadError> {
        let session = self
            .progress
            .get(upload_id)
            .filter(|session| session.info().account == account)
            .ok_or(UploadError::UploadNotFound)?;
        if self.clock.now_utc() >= session.expires_at() {
            self.progress.remove(upload_id);
            return Err(UploadError::UploadNotFound);
        }
        Ok(session)
    }

    async fn write_durable(
        &self,
        account: Uuid,
        handle: &BlobHandle,
        index: u32,
        block: Vec<u8>,
    ) -> Result<(), UploadError> {
        let token = self
            .writer
            .write_block(handle, index, block.into())
            .await
            .map_err(|error| write_error(account, error))?;
        self.writer
            .await_durable(token)
            .await
            .map_err(|error| write_error(account, error))
    }

    /// Records `bytes` acknowledged bytes and, when a checkpoint is due, writes it in one short transaction
    /// that first checks the account. A row that is gone (aborted, reaped, committed or expired) drops the
    /// entry and answers `UploadNotFound`.
    async fn checkpoint_if_due(
        &self,
        session: &SharedProgress,
        upload_id: Uuid,
        bytes: u64,
    ) -> Result<(), UploadError> {
        let policy = CheckpointPolicy {
            bytes: self.config.checkpoint_bytes,
            interval: self.config.checkpoint_interval,
        };
        let at = self.clock.monotonic();
        let Some(taken) = session.checkpoint().record(bytes, at, policy) else {
            return Ok(());
        };
        let run = CheckpointRun {
            session,
            taken,
            open: true,
        };
        let (received, block_hashes) = {
            let progress = session.lock();
            (
                progress.received_bytes().to_vec(),
                progress.block_hash_bytes().to_vec(),
            )
        };
        let now = self.clock.now_utc();
        let expires_at = now
            .checked_add(self.config.upload_session_ttl)
            .ok_or_else(|| UploadError::Unexpected(Box::new(BlockFault::ExpiryOverflow)))?;
        let account = session.info().account;
        let row = Progress {
            id: upload_id,
            account,
            received: &received,
            block_hashes: &block_hashes,
            now,
            expires_at,
        };
        let written = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await?;
                session_repo::checkpoint(tx, &row).await.map_err(unexpected)
            })
            .await
            .map_err(run_error)?;
        if written == 0 {
            self.progress.remove(upload_id);
            return Err(UploadError::UploadNotFound);
        }
        run.succeeded(CheckpointMark { at, expires_at });
        Ok(())
    }
}

/// Reads the whole body; it must hold exactly `expected` bytes. A body that ends early, runs on or fails
/// while being sent is `LengthMismatch`.
async fn read_block(mut body: ByteStream, expected: u64) -> Result<Vec<u8>, UploadError> {
    let capacity = usize::try_from(expected).map_err(|_| UploadError::LengthMismatch)?;
    let mut block = Vec::with_capacity(capacity);
    while let Some(chunk) = std::future::poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        let chunk = chunk.map_err(body_error)?;
        if chunk.len() > capacity - block.len() {
            return Err(UploadError::LengthMismatch);
        }
        block.extend_from_slice(&chunk);
    }
    if block.len() != capacity {
        return Err(UploadError::LengthMismatch);
    }
    Ok(block)
}

fn body_error(error: StreamError) -> UploadError {
    match error {
        StreamError::Source(_) => UploadError::LengthMismatch,
        corrupt @ StreamError::Corrupt => {
            UploadError::Unexpected(Box::new(BlockFault::Body(corrupt)))
        }
    }
}

/// 04's refusals of a block write: a full volume, a block already holding other bytes (04 `write_block_once`),
/// anything else a storage failure (logged; the block is not acknowledged).
fn write_error(account: Uuid, error: BlobError) -> UploadError {
    match error {
        BlobError::NoSpace => UploadError::StorageFull,
        BlobError::BlockConflict => UploadError::BlockConflict,
        other => {
            log_error!("block write failed", account_id = account, err = &other);
            UploadError::Storage(Box::new(other))
        }
    }
}

/// The block's lane: released when the block is not received (refusal, failure or a dropped request).
struct Lane<'a> {
    session: &'a SharedProgress,
    index: u32,
    held: bool,
}

impl Lane<'_> {
    /// Sets the received bit and hash; `true` for a block not received before.
    fn received(mut self, hash: Hash32) -> Result<bool, UploadError> {
        let marked = self.session.lock().mark_received(self.index, hash);
        self.held = marked.is_err();
        marked.map_err(|error| UploadError::Unexpected(Box::new(error)))
    }
}

impl Drop for Lane<'_> {
    fn drop(&mut self) {
        if !self.held {
            return;
        }
        if let Err(error) = self.session.lock().release_block(self.index) {
            log_error!("block lane not released", err = &error);
        }
    }
}

/// A running checkpoint: when it does not succeed (failure or a dropped request) its bytes count again.
struct CheckpointRun<'a> {
    session: &'a SharedProgress,
    taken: u64,
    open: bool,
}

impl CheckpointRun<'_> {
    fn succeeded(mut self, mark: CheckpointMark) {
        self.open = false;
        self.session.checkpoint().succeeded(mark);
    }
}

impl Drop for CheckpointRun<'_> {
    fn drop(&mut self) {
        if self.open {
            self.session.checkpoint().failed(self.taken);
        }
    }
}
