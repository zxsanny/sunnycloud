//! `start_upload`: a large upload session over 2 MiB blocks (`upload_service.md` 1.2.0). Checks run in the
//! contract's order, reading only: size, name, parent (with the vault file limit when the parent is in a vault),
//! then the `NothingToSend` rule (the committed key first,
//! then the tree cases). One short transaction then reserves the quota (taking the quota row lock, which
//! serialises the session cap of concurrent starts), checks the cap and inserts the `blobs` and
//! `upload_sessions` rows. The file is created after COMMIT; when that fails a compensating short transaction
//! removes both rows and releases the reservation. 04 checks the free-space floor inside `create_file`, so a
//! start below the floor is refused after its transaction and compensated: nothing stays reserved.

use std::sync::Arc;

use contracts::{
    AccountGuard, AccountGuardError, BlobCatalog, BlobError, BlobPlan, BlobReader, BlobRegistry,
    BlobWriter, CommitEffect, Fault, QuotaAdmin, QuotaLedger, QuotaLedgerError, RegisterState,
    StartUpload, StartUploadOutcome, Started, TreeCommit, TreeReader, UploadError,
    WrittenOrPlanned,
};
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sc_ids::new_id;
use sc_names::validate_name;
use sc_observability::{Metrics, ObsError, log_error, log_warn};
use thiserror::Error;
use uuid::Uuid;

use crate::blocks::{BLOCK_SIZE_U32, as_u32, block_count, need_blocks};
use crate::caps::InflightCaps;
use crate::checkpoint::CheckpointMark;
use crate::config::TransferConfig;
use crate::progress::{ProgressTable, SessionInfo};
use crate::session_repo::{self, CLIENT_KEY_INDEX, KeyRow, NewSession};
use crate::shortcut;
use crate::upload_metrics::{Rejection, UploadMetrics};
use crate::vault_limit::within_vault_limit;

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

/// The upload sessions, staged uploads and commits of 03 (`UploadService`, in `service`; `commit_upload`, in
/// `commit`). `progress` holds the
/// sessions started or touched by this process (decision 12: one API process), loaded lazily from their checkpoint
/// row; `caps` the account and buffer in-flight caps.
pub struct PgUploadService {
    pub(crate) db: Db,
    pub(crate) config: TransferConfig,
    guard: Arc<dyn AccountGuard>,
    pub(crate) tree: Arc<dyn TreeReader>,
    pub(crate) commit: Arc<dyn TreeCommit>,
    pub(crate) reader: Arc<dyn BlobReader>,
    pub(crate) commit_effects: Vec<Arc<dyn CommitEffect>>,
    pub(crate) quota: Arc<dyn QuotaLedger>,
    pub(crate) quota_admin: Arc<dyn QuotaAdmin>,
    pub(crate) writer: Arc<dyn BlobWriter>,
    pub(crate) registry: Arc<dyn BlobRegistry>,
    pub(crate) catalog: Arc<dyn BlobCatalog>,
    pub(crate) clock: Arc<dyn Clock>,
    /// Shared with the download bodies that count their bytes.
    pub(crate) metrics: Option<Arc<UploadMetrics>>,
    pub(crate) progress: ProgressTable,
    pub(crate) caps: InflightCaps,
}

/// The ports of the upload sessions and commits: the account guard (01), the tree reader and `TreeCommit` (02),
/// the quota ledger and its view (07a; `stage_small` pre-checks the free room with `QuotaAdmin::view`), the blob
/// writer, reader, registry and catalog (04; the catalog serves an uploading blob's plan to the lazy load, the
/// reader the head commit sniffs) and the commit effects the composition root injects (08's media effect), run
/// after 03's own.
pub struct UploadPorts {
    pub guard: Arc<dyn AccountGuard>,
    pub tree: Arc<dyn TreeReader>,
    pub commit: Arc<dyn TreeCommit>,
    pub reader: Arc<dyn BlobReader>,
    pub commit_effects: Vec<Arc<dyn CommitEffect>>,
    pub quota: Arc<dyn QuotaLedger>,
    pub quota_admin: Arc<dyn QuotaAdmin>,
    pub writer: Arc<dyn BlobWriter>,
    pub registry: Arc<dyn BlobRegistry>,
    pub catalog: Arc<dyn BlobCatalog>,
}

/// Why the session (or staged row) transaction did not create its row.
pub(crate) enum TxOutcome {
    Refused(UploadError),
    /// A concurrent start with the same key committed first; the key is read again.
    KeyTaken,
}

impl PgUploadService {
    pub fn new(db: Db, config: TransferConfig, ports: UploadPorts, clock: Arc<dyn Clock>) -> Self {
        let UploadPorts {
            guard,
            tree,
            commit,
            reader,
            commit_effects,
            quota,
            quota_admin,
            writer,
            registry,
            catalog,
        } = ports;
        let caps = InflightCaps::new(
            config.max_inflight_blocks_per_account,
            config.max_buffered_block_bytes,
        );
        PgUploadService {
            db,
            config,
            guard,
            tree,
            commit,
            reader,
            commit_effects,
            quota,
            quota_admin,
            writer,
            registry,
            catalog,
            clock,
            metrics: None,
            progress: ProgressTable::new(),
            caps,
        }
    }

    /// Registers the upload series of [`UploadMetrics`]; `Duplicate` when already registered.
    pub fn with_metrics(mut self, metrics: &Metrics) -> Result<Self, ObsError> {
        self.metrics = Some(Arc::new(UploadMetrics::register(metrics)?));
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
        let in_vault =
            shortcut::check_parent(self.tree.as_ref(), account, request.parent_id).await?;
        if in_vault && !within_vault_limit(size, self.config.max_vault_file_bytes) {
            return Err(UploadError::VaultFileTooLarge);
        }
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
        let handle = match self.writer.create_file(plan, size, true).await {
            Ok(handle) => handle,
            Err(error) => {
                self.compensate(account, started.upload_id, blob_id, size)
                    .await;
                return Err(self.refused(account, blob_error(error)));
            }
        };
        let info = SessionInfo {
            account,
            declared_size: size,
            handle,
        };
        let mark = CheckpointMark {
            at: self.clock.monotonic(),
            expires_at: started.expires_at,
        };
        self.progress
            .insert_new(started.upload_id, info, as_u32(block_count), mark);
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
    /// same declared size is resumed, another size is `ClientKeyMismatch`. An expired row (committed, open or
    /// staged), or no row, starts a session; the session transaction reaps the expired row first.
    fn answer_from_key(
        &self,
        row: Option<&KeyRow>,
        declared_size: i64,
    ) -> Result<Option<StartUploadOutcome>, UploadError> {
        let Some(row) = row else {
            return Ok(None);
        };
        if session_repo::is_expired(row.expires_at, self.clock.now_utc()) {
            return Ok(None);
        }
        match row.committed_revision_id {
            Some(revision) => Ok(Some(StartUploadOutcome::NothingToSend { revision })),
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
        // A row under the key that did not answer is expired (`answer_from_key`); the deleter settles it.
        let stale = known.is_some();
        let result = self
            .db
            .run(Scope::Account(account), async |tx| {
                self.ensure_active(tx, account).await.map_err(stop)?;
                let mut reaped = Vec::new();
                if stale {
                    reaped = self
                        .reap_in_tx(tx, account, Some(&request.client_key), now, 1)
                        .await
                        .map_err(|fault| stop(UploadError::Unexpected(fault)))?;
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
                session_repo::insert(tx, &session)
                    .await
                    .map_err(key_taken)?;
                Ok(reaped)
            })
            .await;
        match result {
            Ok(reaped) => {
                self.reaped(account, &reaped).await;
                Ok(self.started(session.id, (0..as_u32(block_count)).collect(), expires_at))
            }
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

    pub(crate) async fn ensure_active(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), UploadError> {
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
    pub(crate) fn refused(&self, account: Uuid, error: UploadError) -> UploadError {
        let reason = match error {
            UploadError::QuotaExceeded => Rejection::Quota,
            UploadError::SessionCapReached => Rejection::SessionCap,
            UploadError::FreeSpaceFloor => Rejection::FreeFloor,
            _ => return error,
        };
        if let Some(metrics) = &self.metrics {
            metrics.rejected(reason);
        }
        log_warn!("upload refused", account_id = account, err = &error);
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

pub(crate) fn stop(error: UploadError) -> TxOutcome {
    TxOutcome::Refused(error)
}

/// The session insert lost the key to a concurrent start; any other failure is unexpected.
pub(crate) fn key_taken(error: DbError) -> TxOutcome {
    match error {
        DbError::UniqueViolation {
            constraint: Some(ref constraint),
        } if constraint == CLIENT_KEY_INDEX => TxOutcome::KeyTaken,
        other => stop(unexpected(other)),
    }
}

pub(crate) fn quota_error(error: QuotaLedgerError) -> UploadError {
    match error {
        QuotaLedgerError::QuotaExceeded { .. } => UploadError::QuotaExceeded,
        other => UploadError::Unexpected(Box::new(other)),
    }
}

pub(crate) fn blob_error(error: BlobError) -> UploadError {
    match error {
        BlobError::FreeSpaceFloor => UploadError::FreeSpaceFloor,
        BlobError::NoSpace => UploadError::StorageFull,
        other => UploadError::Storage(Box::new(other)),
    }
}

pub(crate) fn unexpected(error: DbError) -> UploadError {
    UploadError::Unexpected(Box::new(error))
}

pub(crate) fn run_error(error: RunError<UploadError>) -> UploadError {
    match error {
        RunError::Closure(error) => error,
        RunError::Db(error) => unexpected(error),
    }
}
