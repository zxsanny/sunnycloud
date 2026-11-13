//! `DerivedOpener` (E-18/c09, `derived_access.md`): a live file to the derived blob of its current content,
//! never to the original. The node goes through 02's `TreeReader`, the derived row and the pending-job probe run
//! in one short account-scoped transaction, and the key material comes from 04's `BlobCatalog` after it closed.

use std::sync::Arc;

use contracts::{
    BlobCatalog, DerivedBlob, DerivedError, DerivedKind, DerivedOpener, LiveFile, LiveFileLookup,
    NodeKind, TreeReader, TreeReaderError, async_trait,
};
use media_shared::types::DerivedKind as OutputKind;
use sc_db::{Db, DbError, Scope};
use uuid::Uuid;

use crate::derived_access_repo::{self, DerivedRow};
use crate::planner::JobPlanner;

pub struct DerivedFiles {
    db: Db,
    tree: Arc<dyn TreeReader>,
    blobs: Arc<dyn BlobCatalog>,
    params_version: u32,
}

/// The lookup outcome inside the transaction.
enum Found {
    Row(DerivedRow),
    Pending,
    Absent,
}

pub(crate) fn output_kind(kind: DerivedKind) -> OutputKind {
    match kind {
        DerivedKind::Thumb => OutputKind::Thumb,
        DerivedKind::Preview => OutputKind::Preview,
        DerivedKind::Transcode => OutputKind::Transcode,
    }
}

impl DerivedFiles {
    /// Derived rows are read at the params version the planner enqueues with.
    pub fn new(
        db: Db,
        tree: Arc<dyn TreeReader>,
        blobs: Arc<dyn BlobCatalog>,
        planner: JobPlanner,
    ) -> Self {
        DerivedFiles {
            db,
            tree,
            blobs,
            params_version: planner.params_version(),
        }
    }

    /// A gone node, a node of another account and a folder are `NotFound`.
    async fn open_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        let file = match self.tree.resolve_live_file(account, node_id).await {
            Ok(LiveFileLookup::Found(file)) => file,
            Ok(LiveFileLookup::Gone) | Err(TreeReaderError::NotFound) => {
                return Err(DerivedError::NotFound);
            }
            Err(error) => return Err(DerivedError::Unexpected(Box::new(error))),
        };
        self.open_file(account, &file, kind).await
    }

    /// The derived blob of `file`'s current content in `account`: the account's own row only, so a file of
    /// another account is `NotFound`. A missing row is `NotReady` while a job that produces the kind is queued
    /// or running and `NotFound` otherwise (no job planned for the type, a vault file, a dead job).
    async fn open_file(
        &self,
        account: Uuid,
        file: &LiveFile,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        let content_hash = match (&file.kind, &file.revision) {
            (NodeKind::File, Some(revision)) => revision.content_hash,
            _ => return Err(DerivedError::NotFound),
        };
        let kind = output_kind(kind);
        let params_version = self.params_version;
        let found = self
            .db
            .run(Scope::Account(account), async |tx| {
                let row = derived_access_repo::derived_row(
                    tx,
                    account,
                    &content_hash,
                    kind,
                    params_version,
                )
                .await?;
                if let Some(row) = row {
                    return Ok::<_, DbError>(Found::Row(row));
                }
                let pending = derived_access_repo::pending_job(
                    tx,
                    account,
                    &content_hash,
                    kind,
                    params_version,
                )
                .await?;
                Ok(if pending {
                    Found::Pending
                } else {
                    Found::Absent
                })
            })
            .await
            .map_err(|error| DerivedError::Unexpected(Box::new(error)))?;
        let row = match found {
            Found::Row(row) => row,
            Found::Pending => return Err(DerivedError::NotReady),
            Found::Absent => return Err(DerivedError::NotFound),
        };
        // `derived` references its blob by a restricting foreign key, so a row without a readable blob breaks
        // an invariant: unexpected, never the original in its place.
        let blob = self
            .blobs
            .get_ref(account, row.blob_id)
            .await
            .map_err(|error| DerivedError::Unexpected(Box::new(error)))?
            .ok_or_else(|| {
                DerivedError::Unexpected("the derived row's blob has no readable content".into())
            })?;
        Ok(DerivedBlob {
            blob,
            media_type: row.media_type,
        })
    }
}

#[async_trait]
impl DerivedOpener for DerivedFiles {
    async fn open_derived(
        &self,
        account: Uuid,
        node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        self.open_node(account, node_id, kind).await
    }

    async fn open_derived_for(
        &self,
        account: Uuid,
        file: &LiveFile,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        self.open_file(account, file, kind).await
    }
}
