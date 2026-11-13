//! Wrappers of the real ports that land an event at one step of `accept_output`: the retention purge collecting
//! the job's revision just before the revision check, and a fault of the first blob registration (the step
//! between the file write and the row transaction's end).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use contracts::{
    BlobRegistry, BlobRegistryError, ListingPage, LiveFileLookup, NodeRef, NodeSelector, NodeView,
    RegisterState, ReinstateOutcome, RevisionMatch, RevisionView, TreeReader, TreeReaderError,
    WrittenOrPlanned, async_trait,
};
use sc_db::DbTx;
use sc_ids::FileHash;
use sqlx::{Connection, PgConnection};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Delegates to the real tree reader; once armed with an owner connection, the next revision check first
/// deletes the revision row as the retention purge does (its jobs go with it by the cascade).
pub struct CollectingTree {
    pub inner: Arc<dyn TreeReader>,
    pub owner: Mutex<Option<PgConnection>>,
}

impl CollectingTree {
    pub async fn arm(&self, conn: PgConnection) {
        *self.owner.lock().await = Some(conn);
    }

    pub async fn close(&self) {
        if let Some(conn) = self.owner.lock().await.take() {
            conn.close().await.expect("close owner connection");
        }
    }
}

#[async_trait]
impl TreeReader for CollectingTree {
    async fn get_node(
        &self,
        account: Uuid,
        selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        self.inner.get_node(account, selector).await
    }

    async fn list_children(
        &self,
        account: Uuid,
        folder_id: Uuid,
        after_name_key: Option<&str>,
        limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        self.inner
            .list_children(account, folder_id, after_name_key, limit)
            .await
    }

    async fn find_live_node(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        self.inner.find_live_node(account, parent_id, name).await
    }

    async fn resolve_live_file(
        &self,
        account: Uuid,
        node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        self.inner.resolve_live_file(account, node_id).await
    }

    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError> {
        self.inner.root_node(account).await
    }

    async fn get_revision(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError> {
        self.inner.get_revision(account, revision_id).await
    }

    async fn is_current_or_retained(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<bool, TreeReaderError> {
        if let Some(conn) = self.owner.lock().await.as_mut() {
            sqlx::query("DELETE FROM revisions WHERE id = $1 AND account_id = $2")
                .bind(revision_id)
                .bind(account)
                .execute(conn)
                .await
                .expect("collect the revision");
        }
        self.inner
            .is_current_or_retained(account, revision_id)
            .await
    }

    async fn describe_blobs(
        &self,
        blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError> {
        self.inner.describe_blobs(blob_ids).await
    }

    async fn lookup_revisions(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError> {
        self.inner.lookup_revisions(account, hashes).await
    }
}

/// The real registry, except that the first registration fails as a database fault would.
pub struct FailingRegistry {
    pub inner: Arc<dyn BlobRegistry>,
    pub fail_next: AtomicBool,
}

#[async_trait]
impl BlobRegistry for FailingRegistry {
    async fn register_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob: WrittenOrPlanned,
        state: RegisterState,
    ) -> Result<(), BlobRegistryError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(BlobRegistryError::Unexpected("injected fault".into()));
        }
        self.inner.register_in_tx(tx, account, blob, state).await
    }

    async fn mark_committed_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
        size: i64,
    ) -> Result<(), BlobRegistryError> {
        self.inner
            .mark_committed_in_tx(tx, account, blob_id, size)
            .await
    }

    async fn reinstate_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<ReinstateOutcome, BlobRegistryError> {
        self.inner.reinstate_in_tx(tx, account, blob_id).await
    }

    async fn remove_row_in_tx(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<(), BlobRegistryError> {
        self.inner.remove_row_in_tx(tx, account, blob_id).await
    }
}
