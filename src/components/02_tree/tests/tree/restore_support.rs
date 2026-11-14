//! The operator restore (AZ-3715, IT-02-21): `PgTreeRestore` over the test tree's service, the real 04
//! `BlobRegistry` on a scratch blob directory, and files whose revisions are published at chosen instants.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{NodeOrPath, RestoreError, RestoreOutcome, TreeCommit, TreeRestore};
use sc_clock::Timestamp;
use sunnycloud_blobstore::{BlobRegistry, blob_path};
use sunnycloud_tree::PgTreeRestore;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::publish_support::actor;
use crate::support::START;
use crate::usage_support::{edit_sized, new_sized, published};

pub const HOUR: Duration = Duration::from_secs(3600);
/// The retention of the restore (and of the test queue): 7 days.
pub const RETENTION: Duration = Duration::from_secs(7 * 86_400);
pub const R1_SIZE: i64 = 100;
pub const R2_SIZE: i64 = 300;

/// `hours` before the start time.
pub fn hours_ago(hours: u64) -> Timestamp {
    START
        .checked_sub(HOUR * hours as u32)
        .expect("an earlier instant")
}

pub fn micros_after(at: Timestamp, micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(at.as_unix_micros() + micros)
}

/// A file with r1 (`R1_SIZE`) published 3 hours ago and r2 (`R2_SIZE`) 2 hours ago, both blobs with files.
pub struct TwoRevisions {
    pub node: Uuid,
    pub r1: Uuid,
    pub r2: Uuid,
    pub blob1: Uuid,
    pub blob2: Uuid,
}

pub struct Restore {
    pub tree: Tree,
    pub dir: PathBuf,
    pub restore: PgTreeRestore,
}

impl Restore {
    pub async fn new() -> Restore {
        let tree = Tree::new().await;
        let dir = std::env::temp_dir().join(format!("sc-tree-restore-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("blob root");
        let config = crate::lifecycle_support::config(&dir);
        let registry =
            BlobRegistry::new(&config, sc_testdb::clock_at_start()).expect("blob registry");
        let restore = PgTreeRestore::new(tree.service.clone(), Arc::new(registry), RETENTION);
        Restore { tree, dir, restore }
    }

    /// A committed blob row of the account with its file in the blob directory.
    pub async fn blob_with_file(&self) -> Uuid {
        let blob = self.tree.blobs(1).await[0];
        let path = blob_path(&self.dir, blob);
        fs::create_dir_all(path.parent().expect("blob folder")).expect("folders");
        fs::write(path, b"x").expect("blob file");
        blob
    }

    pub fn drop_file(&self, blob: Uuid) {
        fs::remove_file(blob_path(&self.dir, blob)).expect("remove the blob file");
    }

    /// A new file under `parent` at `at`: its node and revision.
    pub async fn create_at(
        &self,
        parent: Uuid,
        name: &str,
        blob: Uuid,
        size: i64,
        at: Timestamp,
    ) -> (Uuid, Uuid) {
        let (_, node, revision) = self
            .publish_at(new_sized(parent, name, blob, size), at)
            .await;
        (node, revision)
    }

    pub async fn publish_at(
        &self,
        entry: contracts::PublishEntry,
        at: Timestamp,
    ) -> (contracts::PublishOutcome, Uuid, Uuid) {
        match self
            .tree
            .service
            .publish(actor(self.tree.account), vec![entry], Vec::new(), at)
            .await
            .expect("publish")
        {
            contracts::PublishResult::Published(results) => published(&results[0]),
            contracts::PublishResult::Replayed(_) => panic!("unexpected replay"),
        }
    }

    /// r1 three hours ago, r2 (on base r1) two hours ago.
    pub async fn two_revisions(&self, parent: Uuid, name: &str) -> TwoRevisions {
        let (blob1, blob2) = (self.blob_with_file().await, self.blob_with_file().await);
        let (node, r1) = self
            .create_at(parent, name, blob1, R1_SIZE, hours_ago(3))
            .await;
        let (_, _, r2) = self
            .publish_at(edit_sized(node, Some(r1), blob2, R2_SIZE), hours_ago(2))
            .await;
        TwoRevisions {
            node,
            r1,
            r2,
            blob1,
            blob2,
        }
    }

    pub async fn delete(&self, node: Uuid) {
        self.tree
            .service
            .delete_node(self.tree.account, node, None)
            .await
            .expect("delete");
    }

    pub async fn by_path(&self, path: &str, at: Timestamp) -> Result<RestoreOutcome, RestoreError> {
        self.restore
            .restore_revision(self.tree.account, NodeOrPath::Path(path.to_owned()), at)
            .await
    }

    pub async fn by_node(&self, node: Uuid, at: Timestamp) -> Result<RestoreOutcome, RestoreError> {
        self.restore
            .restore_revision(self.tree.account, NodeOrPath::Node(node), at)
            .await
    }

    /// `(parent, name, live, current revision)` of a node.
    pub async fn node(&self, id: Uuid) -> (Option<Uuid>, String, bool, Option<Uuid>) {
        let mut admin = self.tree.fixture.db.admin().await;
        let row = sqlx::query_as(
            "SELECT parent_id, name, deleted_at IS NULL, current_revision_id FROM nodes WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut admin)
        .await
        .expect("read node");
        close(admin).await;
        row
    }

    pub async fn revision_node(&self, revision: Uuid) -> Uuid {
        let mut admin = self.tree.fixture.db.admin().await;
        let node = sqlx::query_scalar("SELECT node_id FROM revisions WHERE id = $1")
            .bind(revision)
            .fetch_one(&mut admin)
            .await
            .expect("read revision");
        close(admin).await;
        node
    }

    /// The `blobs.state` of a blob.
    pub async fn blob_state(&self, blob: Uuid) -> String {
        let mut admin = self.tree.fixture.db.admin().await;
        let state = sqlx::query_scalar("SELECT state FROM blobs WHERE id = $1")
            .bind(blob)
            .fetch_one(&mut admin)
            .await
            .expect("read blob state");
        close(admin).await;
        state
    }

    /// Every row of the account a restore can touch (`Tree::rows_of`).
    pub async fn snapshot(&self) -> String {
        self.tree.rows_of(self.tree.account).await
    }

    /// Removes the blob's `blobs` and `blob_gc` rows behind the revision's back (foreign keys off), so the
    /// registry finds no row to reinstate.
    pub async fn remove_blob_rows(&self, blob: Uuid) {
        let mut admin = self.tree.fixture.db.admin().await;
        sqlx::raw_sql("SET session_replication_role = replica")
            .execute(&mut admin)
            .await
            .expect("disable foreign-key triggers");
        for sql in [
            "DELETE FROM blob_gc WHERE blob_id = $1",
            "DELETE FROM blobs WHERE id = $1",
        ] {
            sqlx::query(sql)
                .bind(blob)
                .execute(&mut admin)
                .await
                .expect("remove blob rows");
        }
        close(admin).await;
    }

    pub async fn finish(self) {
        let Restore { tree, dir, restore } = self;
        drop(restore);
        if let Err(error) = fs::remove_dir_all(&dir) {
            eprintln!("scratch {} not removed: {error}", dir.display());
        }
        tree.finish().await;
    }
}
