//! `contracts::BlobRegistry`: `blobs` row writes inside the caller's transaction. None of these opens a
//! transaction, and every statement is filtered by the account the caller names.

mod registry_repo;

use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use contracts::{
    BlobError, BlobRegistryError, RegisterState, ReinstateOutcome, WrittenOrPlanned, async_trait,
};
use sc_clock::Clock;
use sc_db::DbTx;
use uuid::Uuid;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, db_fault, io_fault, row_fault};
use crate::path::blob_path;
use registry_repo::NewRow;

pub struct BlobRegistry {
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

fn ids(account: Uuid, blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: Some(account),
    }
}

impl BlobRegistry {
    /// `config.blob_root` is required: `reinstate_in_tx` checks the file.
    pub fn new(config: &BlobstoreConfig, clock: Arc<dyn Clock>) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                crate::error::blob_error_from_io(
                    std::io::Error::from(ErrorKind::InvalidInput),
                    BlobIds::default(),
                )
            })?;
        Ok(BlobRegistry {
            root: PathBuf::from(root),
            clock,
        })
    }
}

#[async_trait]
impl contracts::BlobRegistry for BlobRegistry {
    /// Inserts the row of a blob for `account`. A planned blob has no size yet, so it can only start
    /// `uploading`; `Committed` with a plan is refused by the schema (`blobs_committed_size_check`).
    async fn register_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob: WrittenOrPlanned,
        state: RegisterState,
    ) -> Result<(), BlobRegistryError> {
        let (plan, size) = match blob {
            WrittenOrPlanned::Written(written) => (written.plan, Some(written.size)),
            WrittenOrPlanned::Planned(plan) => (plan, None),
        };
        let state = match state {
            RegisterState::Uploading => "uploading",
            RegisterState::Committed => "committed",
        };
        let blob_ids = ids(account, plan.blob_id);
        let row = NewRow {
            id: plan.blob_id,
            account,
            state,
            size,
            wrapped_key: &plan.wrapped_key,
            master_key_id: plan.master_key_id,
            created_micros: self.clock.now_utc().as_unix_micros(),
        };
        registry_repo::insert(tx, row)
            .await
            .map_err(|error| BlobRegistryError::Unexpected(db_fault(error, blob_ids)))
    }

    /// `uploading` to `committed` with the final size. An absent row (or one in another state) is a failure,
    /// so the caller's transaction rolls back.
    async fn mark_committed_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
        size: i64,
    ) -> Result<(), BlobRegistryError> {
        match registry_repo::commit(tx, account, blob_id, size).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(BlobRegistryError::Unexpected(row_fault(
                "no uploading row",
                ids(account, blob_id),
            ))),
            Err(error) => Err(BlobRegistryError::Unexpected(db_fault(
                error,
                ids(account, blob_id),
            ))),
        }
    }

    /// The file is statted before any statement. No row is `BeyondRetention` (the sweeper removed it); a row
    /// whose file is absent is `FileMissing` and stays as it is; otherwise the queue row is removed and the
    /// state returns to `committed`. A row still `uploading` is not restorable and is a failure.
    async fn reinstate_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<ReinstateOutcome, BlobRegistryError> {
        let present = match tokio::fs::metadata(blob_path(&self.root, blob_id)).await {
            Ok(_) => true,
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => {
                return Err(BlobRegistryError::Unexpected(io_fault(
                    &error,
                    ids(account, blob_id),
                )));
            }
        };
        let fail = |error| BlobRegistryError::Unexpected(db_fault(error, ids(account, blob_id)));
        let state = registry_repo::lock_state(tx, account, blob_id)
            .await
            .map_err(fail)?;
        match state.as_deref() {
            None => Ok(ReinstateOutcome::BeyondRetention),
            Some(_) if !present => Ok(ReinstateOutcome::FileMissing),
            Some("gc_pending") => {
                registry_repo::restore(tx, account, blob_id)
                    .await
                    .map_err(fail)?;
                Ok(ReinstateOutcome::Reinstated)
            }
            Some("committed") => Ok(ReinstateOutcome::Reinstated),
            Some(_) => Err(BlobRegistryError::Unexpected(row_fault(
                "row not restorable",
                ids(account, blob_id),
            ))),
        }
    }

    /// Deletes the `blobs` row and, with it, its `blob_gc` row. An absent row is success.
    async fn remove_row_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<(), BlobRegistryError> {
        registry_repo::delete(tx, account, blob_id)
            .await
            .map_err(|error| BlobRegistryError::Unexpected(db_fault(error, ids(account, blob_id))))
    }
}
