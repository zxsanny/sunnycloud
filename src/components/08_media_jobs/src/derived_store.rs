//! `DerivedStore` (E-18/c07): a worker output becomes an encrypted derived blob, at most one per (account, content
//! hash, kind, params version). The checks that need no bytes run first; the body then streams through 04's
//! writer with no transaction open; the stored head is sniffed; one short account-scoped transaction registers
//! the blob, inserts the `derived` row and closes the job when its last expected output arrived. A file that
//! ends up unused is removed: discarded at once when nothing references it, queued `Due::Now` once registered.
//! `open_input` hands the holder of a running job the whole decrypted original through 04's reader, the same
//! decrypt path as downloads.

use std::sync::Arc;

use contracts::{
    AcceptOutcome, BlobCatalog, BlobError, BlobReader, BlobRegistry, BlobRegistryError, BlobWriter,
    ByteStream, DerivedKind, Due, Fault, GcQueue, GcQueueError, GcReason, OutputMeta,
    RegisterState, TreeReader, TreeReaderError, WorkerInput, WorkerJobError, WrittenBlob,
    WrittenOrPlanned,
};
use media_shared::policy::{OutputSpec, Params};
use sc_clock::Clock;
use sc_db::{Db, DbError, RunError, Scope};
use sc_observability::{log_error, log_warn};
use thiserror::Error;
use uuid::Uuid;

use crate::config::MediaConfig;
use crate::derived_opener::output_kind;
use crate::derived_repo::{self, NewDerived};
use crate::job_output_repo;
use crate::job_repo::{self, HeldJob};
use crate::output_rules::{self, Dimensions, OutputCaps};
use crate::sniff::{self, HEAD_LEN};

/// The 04 ports the store writes, reads and registers through.
pub struct BlobPorts {
    pub writer: Arc<dyn BlobWriter>,
    pub reader: Arc<dyn BlobReader>,
    pub catalog: Arc<dyn BlobCatalog>,
    pub registry: Arc<dyn BlobRegistry>,
    pub gc: Arc<dyn GcQueue>,
}

pub struct DerivedStore {
    db: Db,
    clock: Arc<dyn Clock>,
    tree: Arc<dyn TreeReader>,
    blobs: BlobPorts,
    caps: OutputCaps,
}

/// Why the row transaction rolled back.
#[derive(Debug, Error)]
enum RowFailure {
    #[error("lease lost")]
    LeaseLost,
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Registry(#[from] BlobRegistryError),
    #[error(transparent)]
    Gc(#[from] GcQueueError),
}

/// What was checked before the body was read.
struct Admitted<'a> {
    job: &'a HeldJob,
    kind: media_shared::types::DerivedKind,
    params: &'static Params,
    dimensions: Dimensions,
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> WorkerJobError {
    WorkerJobError::Unexpected(Box::new(error))
}

fn storage(error: BlobError) -> WorkerJobError {
    WorkerJobError::Storage(Box::new(error))
}

impl DerivedStore {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        tree: Arc<dyn TreeReader>,
        blobs: BlobPorts,
        config: &MediaConfig,
    ) -> Self {
        DerivedStore {
            db,
            clock,
            tree,
            blobs,
            caps: OutputCaps::from_config(config),
        }
    }

    /// `Accepted` stores the output; `AlreadyPresent` means the key already had its file (a retry or a race), and
    /// the new file is queued for removal. A refused output keeps no file and no committed blob row.
    pub async fn accept_output(
        &self,
        job_id: Uuid,
        worker_id: &str,
        kind: DerivedKind,
        meta: OutputMeta,
        body: ByteStream,
        length: u64,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        let job = self.held(job_id, worker_id, true).await?;
        let (admitted, size) = self.admit(&job, kind, &meta, length)?;
        self.check_revision(&job).await?;
        let written = match self
            .blobs
            .writer
            .write_complete(job.account, size, body)
            .await
        {
            Ok(written) => written,
            Err(BlobError::SizeMismatch) => {
                log_warn!("worker output refused: body length differs from the declared length");
                return Err(WorkerJobError::OutputTooLarge);
            }
            Err(error) => return Err(storage(error)),
        };
        let blob_id = written.plan.blob_id;
        let media_type = match self.sniffed_type(&admitted, &written).await {
            Ok(media_type) => media_type,
            Err(error) => {
                self.discard(blob_id).await;
                return Err(error);
            }
        };
        self.store_row(job_id, worker_id, &admitted, written, media_type)
            .await
    }

    /// The whole plaintext of the job's revision with its exact length; `Range` plays no part. Only the holder of
    /// the running job gets it.
    pub async fn open_input(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<WorkerInput, WorkerJobError> {
        let job = self.held(job_id, worker_id, false).await?;
        self.check_revision(&job).await?;
        let revision = match self.tree.get_revision(job.account, job.revision).await {
            Ok(revision) => revision,
            Err(TreeReaderError::NotFound) => return Err(WorkerJobError::RevisionObsolete),
            Err(error) => return Err(unexpected(error)),
        };
        let blob = self
            .blobs
            .catalog
            .get_ref(job.account, revision.blob_id)
            .await
            .map_err(unexpected)?
            .ok_or_else(|| WorkerJobError::Unexpected("the revision's blob has no row".into()))?;
        let length = u64::try_from(blob.size)
            .map_err(|_| WorkerJobError::Unexpected("negative blob size".into()))?;
        let body = self
            .blobs
            .reader
            .open(
                blob.blob_id,
                &blob.wrapped_key,
                blob.master_key_id,
                blob.size,
                None,
            )
            .await
            .map_err(storage)?;
        Ok(WorkerInput { length, body })
    }

    /// The job `worker_id` holds; `closed_derived_ok` also admits one it closed `derived`.
    async fn held(
        &self,
        job_id: Uuid,
        worker_id: &str,
        closed_derived_ok: bool,
    ) -> Result<HeldJob, WorkerJobError> {
        let now = self.clock.now_utc();
        job_repo::held(&self.db, job_id, worker_id, now, closed_derived_ok)
            .await
            .map_err(unexpected)?
            .ok_or_else(|| {
                log_warn!("worker job call refused: lease lost");
                WorkerJobError::LeaseLost
            })
    }

    /// The checks that need no body: the kind, the declared length and the reported dimensions.
    fn admit<'a>(
        &self,
        job: &'a HeldJob,
        kind: DerivedKind,
        meta: &OutputMeta,
        length: u64,
    ) -> Result<(Admitted<'a>, i64), WorkerJobError> {
        let kind = output_kind(kind);
        if !job.kind.outputs().contains(&kind) {
            return Err(WorkerJobError::OutputKindRejected);
        }
        let params = Params::for_version(job.params_version)
            .ok_or_else(|| WorkerJobError::Unexpected("job of an unknown params version".into()))?;
        let size = self.caps.admit(kind, length).ok_or_else(|| {
            log_warn!("worker output refused: over the size cap");
            WorkerJobError::OutputTooLarge
        })?;
        let dimensions = output_rules::bounded_dimensions(
            kind,
            params,
            meta.width,
            meta.height,
            meta.duration_ms,
        )
        .ok_or_else(|| {
            log_warn!("worker output refused: dimensions out of bounds");
            WorkerJobError::OutputTypeRejected
        })?;
        let admitted = Admitted {
            job,
            kind,
            params,
            dimensions,
        };
        Ok((admitted, size))
    }

    /// A collected revision is obsolete. Its job row goes with it (`ON DELETE CASCADE`), so this answers only
    /// when the collection lands after the holder check.
    async fn check_revision(&self, job: &HeldJob) -> Result<(), WorkerJobError> {
        match self
            .tree
            .is_current_or_retained(job.account, job.revision)
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) | Err(TreeReaderError::NotFound) => Err(WorkerJobError::RevisionObsolete),
            Err(error) => Err(unexpected(error)),
        }
    }

    /// The type read from the stored plaintext, if the kind accepts it.
    async fn sniffed_type(
        &self,
        admitted: &Admitted<'_>,
        written: &WrittenBlob,
    ) -> Result<&'static str, WorkerJobError> {
        let head = self
            .blobs
            .reader
            .read_head(
                written.plan.blob_id,
                &written.plan.wrapped_key,
                written.plan.master_key_id,
                written.size,
                HEAD_LEN,
            )
            .await
            .map_err(storage)?;
        let spec = OutputSpec::for_kind(admitted.kind, admitted.params);
        sniff::sniff(&head)
            .filter(|media_type| spec.accepts(media_type))
            .ok_or_else(|| {
                log_warn!("worker output refused by the sniffer");
                WorkerJobError::OutputTypeRejected
            })
    }

    /// The one short transaction. A rolled-back transaction leaves the file unreferenced, so it is discarded; when
    /// the commit itself failed its outcome is unknown and the file is left to 04's orphan sweep.
    async fn store_row(
        &self,
        job_id: Uuid,
        worker_id: &str,
        admitted: &Admitted<'_>,
        written: WrittenBlob,
        media_type: &'static str,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        let job = admitted.job;
        let blob_id = written.plan.blob_id;
        let now = self.clock.now_utc();
        let row = NewDerived {
            account: job.account,
            content_hash: &job.content_hash,
            kind: admitted.kind,
            params_version: job.params_version,
            blob_id,
            media_type,
            size: written.size,
            width: admitted.dimensions.width,
            height: admitted.dimensions.height,
            duration_ms: admitted.dimensions.duration_ms,
            created_at: now,
        };
        let result = self
            .db
            .run(Scope::Account(job.account), async |tx| {
                let Some(running) =
                    job_output_repo::lock_for_output(tx, job.account, job_id, worker_id, now)
                        .await?
                else {
                    return Err(RowFailure::LeaseLost);
                };
                let blob = WrittenOrPlanned::Written(written);
                self.blobs
                    .registry
                    .register_in_tx(tx, job.account, blob, RegisterState::Committed)
                    .await?;
                if !derived_repo::insert(tx, &row).await? {
                    self.blobs
                        .gc
                        .schedule_in_tx(tx, blob_id, GcReason::DerivedRemoved, Due::Now)
                        .await?;
                    return Ok(AcceptOutcome::AlreadyPresent);
                }
                if running {
                    let present = derived_repo::kinds_present(
                        tx,
                        job.account,
                        &job.content_hash,
                        job.params_version,
                    )
                    .await?;
                    if output_rules::all_outputs_present(job.kind, &present) {
                        job_output_repo::close_derived(tx, job.account, job_id, now).await?;
                    }
                }
                Ok(AcceptOutcome::Accepted)
            })
            .await;
        match result {
            Ok(outcome) => Ok(outcome),
            Err(RunError::Closure(failure)) => {
                self.discard(blob_id).await;
                Err(match failure {
                    RowFailure::LeaseLost => {
                        log_warn!("worker output refused: lease lost");
                        WorkerJobError::LeaseLost
                    }
                    other => unexpected(other),
                })
            }
            Err(RunError::Db(error)) => {
                let fault: Fault = Box::new(error);
                log_error!(
                    "derived row transaction failed; the file is left to the orphan sweep",
                    err = &*fault
                );
                Err(WorkerJobError::Unexpected(fault))
            }
        }
    }

    /// Best effort: a file that cannot be removed now has no row and is removed by 04's orphan sweep.
    async fn discard(&self, blob_id: Uuid) {
        if let Err(error) = self.blobs.writer.discard(blob_id).await {
            log_error!(
                "refused derived file not removed; left to the orphan sweep",
                err = &error
            );
        }
    }
}
