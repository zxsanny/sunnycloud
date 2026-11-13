//! `start_upload`: a large upload session over 2 MiB blocks (`upload_service.md` 1.2.0). Checks run in the
//! contract's order, reading only: size, name, parent, then the `NothingToSend` rule (the committed key first,
//! then the tree cases). One short transaction then reserves the quota (taking the quota row lock, which
//! serialises the session cap of concurrent starts), checks the cap and inserts the `blobs` and
//! `upload_sessions` rows. The file is created after COMMIT; when that fails a compensating short transaction
//! removes both rows and releases the reservation. 04 checks the free-space floor inside `create_file`, so a
//! start below the floor is refused after its transaction and compensated: nothing stays reserved.

use std::sync::Arc;

use contracts::{
    AccountGuard, AccountGuardError, BlobError, BlobPlan, BlobRegistry, BlobWriter, Fault,
    QuotaLedger, QuotaLedgerError, RegisterState, StartUpload, StartUploadOutcome, Started,
    TreeReader, UploadError, WrittenOrPlanned,
};
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sc_ids::{BLOCK_SIZE, new_id};
use sc_names::validate_name;
use sc_observability::{Metrics, ObsError, log_error, log_warn};
use thiserror::Error;
use uuid::Uuid;

use crate::config::TransferConfig;
use crate::session_repo::{self, CLIENT_KEY_INDEX, KeyRow, NewSession};
use crate::shortcut;
use crate::upload_metrics::{Rejection, UploadMetrics};

/// The 2 MiB protocol block size as the port's `block_size`.
const BLOCK_SIZE_U32: u32 = BLOCK_SIZE as u32;

/// The longest `client_key` the session row stores (`upload_sessions_client_key_check`).
const MAX_CLIENT_KEY_BYTES: usize = 256;

/// Unexpected failures of a start. The first two are requests the delivery layer should have refused (tier 1):
/// a large upload of at most 0 bytes (a zero-byte file goes through `stage_small`) or a `client_key` outside
/// 1..=256 bytes; reaching 03 with one is a bug.
#[derive(Debug, Error)]
pub enum StartFault {
    #[error("declared size {0} is not positive; a zero-byte file is staged")]
    DeclaredSize(i64),
    #[error("client key is {0} bytes, 1 to 256 allowed")]
    ClientKey(usize),
    #[error("session expiry overflows the timestamp range")]
    ExpiryOverflow,
}

/// The upload sessions of 03. `UploadService` itself is implemented once blocks, state and staging exist.
pub struct PgUploadService {
    db: Db,
    config: TransferConfig,
    guard: Arc<dyn AccountGuard>,
    tree: Arc<dyn TreeReader>,
    quota: Arc<dyn QuotaLedger>,
    writer: Arc<dyn BlobWriter>,
    registry: Arc<dyn BlobRegistry>,
    clock: Arc<dyn Clock>,
    metrics: Option<UploadMetrics>,
}

/// The ports `start_upload` calls: the account guard (01), the tree reader (02), the quota ledger (07a) and the
/// blob writer and registry (04).
pub struct UploadPorts {
    pub guard: Arc<dyn AccountGuard>,
    pub tree: Arc<dyn TreeReader>,
    pub quota: Arc<dyn QuotaLedger>,
    pub writer: Arc<dyn BlobWriter>,
    pub registry: Arc<dyn BlobRegistry>,
}

/// Why the session transaction did not create a session.
enum TxOutcome {
    Refused(UploadError),
    /// A concurrent start with the same key committed first; the key is read again.
    KeyTaken,
}

impl PgUploadService {
    pub fn new(db: Db, config: TransferConfig, ports: UploadPorts, clock: Arc<dyn Clock>) -> Self {
        let UploadPorts {
            guard,
            tree,
            quota,
            writer,
            registry,
        } = ports;
        PgUploadService {
            db,
            config,
            guard,
            tree,
            quota,
            writer,
            registry,
            clock,
            metrics: None,
        }
    }

    /// Registers `upload_sessions_active` and `upload_rejections_total`; `Duplicate` when already registered.
    pub fn with_metrics(mut self, metrics: &Metrics) -> Result<Self, ObsError> {
        self.metrics = Some(UploadMetrics::register(metrics)?);
        Ok(self)
    }

    pub async fn start_upload(
        &self,
        account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        let size = request.declared_size;
        if u64::try_from(size).is_ok_and(|size| size > self.config.max_declared_size) {
            return Err(UploadError::SizeTooLarge);
        }
        check_request(&request)?;
        let name = validate_name(&request.name).map_err(|_| UploadError::InvalidName)?;
        shortcut::check_parent(self.tree.as_ref(), account, request.parent_id).await?;
        // Committed-key case of the `NothingToSend` rule and the resume by key, before the tree cases.
        let mut known = self.read_key(account, &request.client_key).await?;
        if let Some(outcome) = self.answer_from_key(known.as_ref(), size)? {
            return Ok(outcome);
        }
        if let Some(hash) = &request.content_hash {
            let tree = self.tree.as_ref();
            let parent = request.parent_id;
            let base = request.base_revision;
            if let Some(revision) =
                shortcut::same_content(tree, account, parent, &name, base, hash).await?
            {
                return Ok(StartUploadOutcome::NothingToSend { revision });
            }
        }
        let block_count = block_count(size)?;
        let plan = self.writer.plan(account, size).map_err(blob_error)?;
        let blob_id = plan.blob_id;
        let started = loop {
            match self
                .open_session(account, &request, block_count, &plan, known.as_ref())
                .await
            {
                Ok(started) => break started,
                Err(TxOutcome::Refused(error)) => return Err(self.refused(account, error)),
                Err(TxOutcome::KeyTaken) => {
                    known = self.read_key(account, &request.client_key).await?;
                    if let Some(outcome) = self.answer_from_key(known.as_ref(), size)? {
                        return Ok(outcome);
                    }
                }
            }
        };
        if let Err(error) = self.writer.create_file(plan, size, true).await {
            self.compensate(account, started.upload_id, blob_id, size)
                .await;
            return Err(self.refused(account, blob_error(error)));
        }
        if let Some(metrics) = &self.metrics {
            metrics.session_opened();
        }
        Ok(StartUploadOutcome::Started(started))
    }

    /// The row the key names, read in its own short transaction behind the account guard.
    async fn read_key(
        &self,
        account: Uuid,
        client_key: &str,
    ) -> Result<Option<KeyRow>, UploadError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await?;
                session_repo::by_key(tx, account, client_key)
                    .await
                    .map_err(unexpected)
            })
            .await
            .map_err(run_error)
    }

    /// A committed key within its TTL answers its revision whatever the name holds now; an open session with the
    /// same declared size is resumed, another size is `ClientKeyMismatch`. An expired committed row, or no row,
    /// starts a session.
    fn answer_from_key(
        &self,
        row: Option<&KeyRow>,
        declared_size: i64,
    ) -> Result<Option<StartUploadOutcome>, UploadError> {
        let Some(row) = row else {
            return Ok(None);
        };
        match row.committed_revision_id {
            Some(revision) if row.expires_at > self.clock.now_utc() => {
                Ok(Some(StartUploadOutcome::NothingToSend { revision }))
            }
            Some(_) => Ok(None),
            None if row.declared_size != declared_size => Err(UploadError::ClientKeyMismatch),
            None => Ok(Some(StartUploadOutcome::Started(self.started(
                row.id,
                need_blocks(row.block_count, &row.received),
                row.expires_at,
            )))),
        }
    }

    /// The session transaction: guard, quota reserve (the row lock), session cap, blob row, session row.
    async fn open_session(
        &self,
        account: Uuid,
        request: &StartUpload,
        block_count: i32,
        plan: &BlobPlan,
        known: Option<&KeyRow>,
    ) -> Result<Started, TxOutcome> {
        let now = self.clock.now_utc();
        let expires_at = now
            .checked_add(self.config.upload_session_ttl)
            .ok_or_else(|| {
                stop(UploadError::Unexpected(Box::new(
                    StartFault::ExpiryOverflow,
                )))
            })?;
        let session = NewSession {
            id: new_id(self.clock.as_ref()),
            account,
            blob_id: plan.blob_id,
            client_key: &request.client_key,
            declared_size: request.declared_size,
            block_count,
            now,
            expires_at,
        };
        let stale_committed = known.is_some_and(|row| row.committed_revision_id.is_some());
        let result = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await.map_err(stop)?;
                if stale_committed {
                    session_repo::delete_expired_committed(tx, account, &request.client_key, now)
                        .await
                        .map_err(|error| stop(unexpected(error)))?;
                }
                self.quota
                    .reserve(tx, account, request.declared_size)
                    .await
                    .map_err(|error| stop(quota_error(error)))?;
                let open = session_repo::count_open_large(tx, account)
                    .await
                    .map_err(|error| stop(unexpected(error)))?;
                if u64::try_from(open).unwrap_or(u64::MAX) >= self.config.max_open_sessions {
                    return Err(stop(UploadError::SessionCapReached));
                }
                self.registry
                    .register_in_tx(
                        tx,
                        account,
                        WrittenOrPlanned::Planned(plan.clone()),
                        RegisterState::Uploading,
                    )
                    .await
                    .map_err(|error| stop(UploadError::Unexpected(Box::new(error))))?;
                session_repo::insert(tx, &session).await.map_err(key_taken)
            })
            .await;
        match result {
            Ok(()) => Ok(self.started(session.id, (0..as_u32(block_count)).collect(), expires_at)),
            Err(RunError::Closure(outcome)) => Err(outcome),
            Err(RunError::Db(error)) => Err(stop(unexpected(error))),
        }
    }

    /// Removes the session and blob rows and releases the reservation after the file could not be created. A
    /// failure here is logged: the rows stay until the reaper, which settles the reservation.
    async fn compensate(&self, account: Uuid, upload_id: Uuid, blob_id: Uuid, size: i64) {
        let result = self
            .db
            .run(Scope::Account(account), async |tx| -> Result<(), Fault> {
                if session_repo::delete_open(tx, account, upload_id).await? == 0 {
                    return Ok(());
                }
                self.registry.remove_row_in_tx(tx, account, blob_id).await?;
                self.quota.release_reservation(tx, account, size).await?;
                Ok(())
            })
            .await;
        if let Err(error) = result {
            log_error!(
                "upload start not compensated",
                account_id = account,
                err = &error
            );
        }
    }

    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), UploadError> {
        self.guard
            .ensure_active(tx, account)
            .await
            .map_err(|error| match error {
                AccountGuardError::AccountNotActive => UploadError::AccountNotActive,
                AccountGuardError::Unexpected(fault) => UploadError::Unexpected(fault),
            })
    }

    fn started(&self, upload_id: Uuid, need_blocks: Vec<u32>, expires_at: Timestamp) -> Started {
        Started {
            upload_id,
            need_blocks,
            block_size: BLOCK_SIZE_U32,
            lane_limit: u32::try_from(self.config.max_lanes).unwrap_or(u32::MAX),
            expires_at,
        }
    }

    /// Counts and logs a quota, cap or floor refusal (ids only); every other error passes through as it is.
    fn refused(&self, account: Uuid, error: UploadError) -> UploadError {
        let reason = match error {
            UploadError::QuotaExceeded => Rejection::Quota,
            UploadError::SessionCapReached => Rejection::SessionCap,
            UploadError::FreeSpaceFloor => Rejection::FreeFloor,
            _ => return error,
        };
        if let Some(metrics) = &self.metrics {
            metrics.rejected(reason);
        }
        log_warn!("upload start refused", account_id = account, err = &error);
        error
    }
}

/// Tier-1 checks the delivery layer owns; see [`StartFault`].
fn check_request(request: &StartUpload) -> Result<(), UploadError> {
    if request.declared_size <= 0 {
        return Err(UploadError::Unexpected(Box::new(StartFault::DeclaredSize(
            request.declared_size,
        ))));
    }
    let key_len = request.client_key.len();
    if key_len == 0 || key_len > MAX_CLIENT_KEY_BYTES {
        return Err(UploadError::Unexpected(Box::new(StartFault::ClientKey(
            key_len,
        ))));
    }
    Ok(())
}

/// `ceil(declared_size / 2 MiB)`; the configured size ceiling keeps it within the `integer` column.
pub(crate) fn block_count(declared_size: i64) -> Result<i32, UploadError> {
    let block = i64::from(BLOCK_SIZE_U32);
    let blocks = declared_size / block + i64::from(declared_size % block > 0);
    i32::try_from(blocks).map_err(|_| UploadError::SizeTooLarge)
}

/// The indexes whose bit is clear in a `received` bitmap (bit `i` is bit `i % 8` of byte `i / 8`).
pub(crate) fn need_blocks(block_count: i32, received: &[u8]) -> Vec<u32> {
    (0..as_u32(block_count))
        .filter(|index| {
            let byte = received.get(*index as usize / 8).copied().unwrap_or(0);
            byte & (1 << (index % 8)) == 0
        })
        .collect()
}

fn as_u32(block_count: i32) -> u32 {
    u32::try_from(block_count).unwrap_or(0)
}

fn stop(error: UploadError) -> TxOutcome {
    TxOutcome::Refused(error)
}

/// The session insert lost the key to a concurrent start; any other failure is unexpected.
fn key_taken(error: DbError) -> TxOutcome {
    match error {
        DbError::UniqueViolation {
            constraint: Some(ref constraint),
        } if constraint == CLIENT_KEY_INDEX => TxOutcome::KeyTaken,
        other => stop(unexpected(other)),
    }
}

fn quota_error(error: QuotaLedgerError) -> UploadError {
    match error {
        QuotaLedgerError::QuotaExceeded { .. } => UploadError::QuotaExceeded,
        other => UploadError::Unexpected(Box::new(other)),
    }
}

fn blob_error(error: BlobError) -> UploadError {
    match error {
        BlobError::FreeSpaceFloor => UploadError::FreeSpaceFloor,
        BlobError::NoSpace => UploadError::StorageFull,
        other => UploadError::Storage(Box::new(other)),
    }
}

fn unexpected(error: DbError) -> UploadError {
    UploadError::Unexpected(Box::new(error))
}

fn run_error(error: RunError<UploadError>) -> UploadError {
    match error {
        RunError::Closure(error) => error,
        RunError::Db(error) => unexpected(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: i64 = 1 << 20;

    #[test]
    fn block_count_is_the_ceiling_of_two_mib_blocks() {
        // Arrange
        let cases = [
            (1, 1),
            (2 * MIB - 1, 1),
            (2 * MIB, 1),
            (2 * MIB + 1, 2),
            (4 * MIB, 2),
            (10 * MIB, 5),
            (10 * MIB + 1, 6),
        ];

        for (size, expected) in cases {
            // Act
            let blocks = block_count(size).expect("block count");

            // Assert
            assert_eq!(blocks, expected, "size {size}");
        }
    }

    #[test]
    fn a_new_session_needs_every_block() {
        // Arrange
        let blocks = block_count(10 * MIB).expect("block count");

        // Act
        let need = need_blocks(blocks, &[0]);

        // Assert
        assert_eq!(need, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn received_bits_are_not_needed_again() {
        // Arrange
        let received = [0b0000_0101, 0b0000_0001];

        // Act
        let need = need_blocks(9, &received);

        // Assert
        assert_eq!(need, vec![1, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_count_beyond_the_integer_column_is_too_large() {
        // Arrange
        let size = (i64::from(i32::MAX) + 1) * i64::from(BLOCK_SIZE_U32);

        // Act
        let result = block_count(size);

        // Assert
        assert!(matches!(result, Err(UploadError::SizeTooLarge)));
    }
}
