//! The revision reads of `TreeReader` (a revision with its blocklist, the lease check of 08, the batched hash
//! lookup of AC-31 and AC-46, the blob paths of `doctor`), the `TreeReader` port itself, and `LiveUsageSource` for
//! the quota drift check (AC-74). Each call is one short transaction; only `describe_blobs` runs in system scope.

use std::collections::HashMap;

use contracts::{
    ListingPage, LiveFileLookup, LiveUsageError, LiveUsageSource, NodeRef, NodeSelector, NodeView,
    RevisionMatch, RevisionView, TreeReader, TreeReaderError, async_trait,
};
use sc_db::DbTx;
use sc_ids::FileHash;
use uuid::Uuid;

use crate::reader::{PgTreeReader, unexpected};
use crate::{maintenance_repo, revision_repo};

/// The largest hash batch of `lookup_revisions` (AC-31: a first desktop sync checks 1,000 hashes at once).
pub const MAX_LOOKUP_HASHES: usize = 1000;

#[async_trait]
impl TreeReader for PgTreeReader {
    async fn get_node(
        &self,
        account: Uuid,
        selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        PgTreeReader::get_node(self, account, selector).await
    }

    async fn list_children(
        &self,
        account: Uuid,
        folder_id: Uuid,
        after_name_key: Option<&str>,
        limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        PgTreeReader::list_children(self, account, folder_id, after_name_key, limit).await
    }

    async fn find_live_node(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        PgTreeReader::find_live_node(self, account, parent_id, name).await
    }

    async fn resolve_live_file(
        &self,
        account: Uuid,
        node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        PgTreeReader::resolve_live_file(self, account, node_id).await
    }

    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError> {
        PgTreeReader::root_node(self, account).await
    }

    /// Any revision row of the account (current or retained), with its blocklist.
    async fn get_revision(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError> {
        self.read(account, async |tx| {
            revision_repo::revision(tx, account, revision_id)
                .await
                .map_err(unexpected)?
                .ok_or(TreeReaderError::NotFound)
        })
        .await
    }

    /// True while the revision row exists: the retention purge and the account erase delete it when collected.
    async fn is_current_or_retained(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<bool, TreeReaderError> {
        self.read(account, async |tx| {
            revision_repo::exists(tx, account, revision_id)
                .await
                .map_err(unexpected)
        })
        .await
    }

    /// System scope, one statement for the whole list. A blob of a live file maps to its path below the root
    /// (`a/b.txt`); a blob whose file or a folder above it is tombstoned maps to no path (proposed default
    /// 2026-11-13: included, marked deleted); an id no revision holds (a random id, a derived blob) is absent.
    async fn describe_blobs(
        &self,
        blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError> {
        if blob_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let places =
            maintenance_repo::in_system(&self.db, "tree_describe_blobs", async |tx: &mut DbTx| {
                revision_repo::describe_blobs(tx, blob_ids)
                    .await
                    .map_err(unexpected)
            })
            .await?;
        Ok(places
            .into_iter()
            .map(|place| (place.blob_id, place.path.into_iter().collect()))
            .collect())
    }

    /// The limit is checked before any query; the batch is one indexed statement.
    async fn lookup_revisions(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError> {
        if hashes.len() > MAX_LOOKUP_HASHES {
            return Err(TreeReaderError::BatchTooLarge);
        }
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        self.read(account, async |tx| {
            revision_repo::lookup(tx, account, hashes)
                .await
                .map_err(unexpected)
        })
        .await
    }
}

#[async_trait]
impl LiveUsageSource for PgTreeReader {
    /// The sizes of the current revisions of files whose own node is live; a file under a tombstoned folder counts
    /// until the cascade marks it (at most one maintenance interval).
    async fn live_bytes(&self, account: Uuid) -> Result<i64, LiveUsageError> {
        self.read(account, async |tx| {
            revision_repo::live_bytes(tx, account)
                .await
                .map_err(unexpected)
        })
        .await
        .map_err(|error| match error {
            TreeReaderError::Unexpected(fault) => LiveUsageError::Unexpected(fault),
            other => LiveUsageError::Unexpected(Box::new(other)),
        })
    }
}
