//! Fixtures of the content tests (AZ-3700): an account whose files change through the real 02, wired as the
//! composition root does (`PgTreeService` with 05's stamp and notifier, the 07a ledger, the 04 `GcQueue` and the
//! shared `StateGuard`; `PgTreeMaintenance` with 05's `PgFeedHorizon`), all on the harness's manual clock. Only the
//! account rows and the committed blob rows a publish consumes are written as the schema owner.

use std::sync::Arc;

use contracts::{
    CommitActor, EntryResult, NewRevision, PublishEntry, PublishOutcome, PublishResult,
    ShareContent, Target, TreeCommit, TreeMaintenance,
};
use sc_clock::Clock;
use sc_ids::{FileHash, Hash32};
use sqlx::{Connection, PgConnection};
use sunnycloud_blobstore::GcQueue;
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use sunnycloud_sync_feed::{InProcessNotifier, PgFeedHorizon, PgFeedStamp};
use sunnycloud_testkit::StateGuard;
use sunnycloud_tree::{PgTreeMaintenance, PgTreeService};
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::account_with_root;

const PURGE_LIMIT: u32 = 100;

/// One published file version: its node and its revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub node: Uuid,
    pub revision: Uuid,
}

/// An account with its root, feed and quota rows, and 02's mutations and maintenance over it.
pub struct Owner {
    pub account: Uuid,
    pub root: Uuid,
    pub tree: PgTreeService,
    maintenance: PgTreeMaintenance,
    probe: PgConnection,
}

impl Owner {
    pub async fn new(harness: &Harness) -> Owner {
        let mut probe = harness.fixture_conn().await;
        let account = Uuid::new_v4();
        let root = account_with_root(&mut probe, account).await;
        for statement in [
            "INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)",
            "INSERT INTO quota (account_id) VALUES ($1)",
        ] {
            sqlx::query(statement)
                .bind(account)
                .execute(&mut probe)
                .await
                .expect("insert account row");
        }
        let queue = Arc::new(
            GcQueue::new(
                &harness.blob_config,
                harness.api.clone(),
                harness.clock.clone(),
            )
            .expect("gc queue"),
        );
        let ledger = Arc::new(PgQuotaLedger::new(QuotaConfig {
            default_quota_bytes: 1 << 40,
        }));
        let tree = PgTreeService::new(
            harness.api.clone(),
            Arc::new(StateGuard::default()),
            ledger.clone(),
            queue.clone(),
            Arc::new(PgFeedStamp),
            InProcessNotifier::new(),
            harness.clock.clone(),
        );
        let maintenance = PgTreeMaintenance::new(
            harness.api.clone(),
            queue,
            ledger,
            Arc::new(PgFeedHorizon),
            &harness.tree_config,
        );
        Owner {
            account,
            root,
            tree,
            maintenance,
            probe,
        }
    }

    pub async fn folder(&self, parent: Uuid, name: &str) -> Uuid {
        self.tree
            .create_folder(self.account, parent, name)
            .await
            .expect("create folder")
            .id
    }

    /// A new file `name` under `parent`, published through 02.
    pub async fn new_file(&mut self, harness: &Harness, parent: Uuid, name: &str) -> Version {
        let target = Target::New {
            parent_id: parent,
            name: name.to_owned(),
        };
        self.publish(harness, target).await
    }

    /// A new revision of `file` on top of `base` (a save in place; on a deleted file, the edit that beats it).
    pub async fn edit(&mut self, harness: &Harness, base: Version) -> (PublishOutcome, Version) {
        let target = Target::Existing {
            node_id: base.node,
            base_revision: Some(base.revision),
        };
        self.publish_with_outcome(harness, target).await
    }

    async fn publish(&mut self, harness: &Harness, target: Target) -> Version {
        self.publish_with_outcome(harness, target).await.1
    }

    async fn publish_with_outcome(
        &mut self,
        harness: &Harness,
        target: Target,
    ) -> (PublishOutcome, Version) {
        let blob = self.committed_blob().await;
        let revision_id = Uuid::new_v4();
        let entry = PublishEntry {
            target,
            revision: new_revision(revision_id, blob),
            vault: false,
        };
        let actor = CommitActor {
            account_id: self.account,
            device_label: "laptop".to_owned(),
        };
        let result = self
            .tree
            .publish(actor, vec![entry], Vec::new(), harness.clock.now_utc())
            .await
            .expect("publish");
        let PublishResult::Published(entries) = result else {
            panic!("a fresh publish is never replayed: {result:?}");
        };
        match entries.as_slice() {
            [
                EntryResult::Published {
                    outcome,
                    node_id,
                    revision_id: published,
                    ..
                },
            ] => {
                assert_eq!(*published, revision_id);
                let version = Version {
                    node: *node_id,
                    revision: *published,
                };
                (*outcome, version)
            }
            other => panic!("one published entry expected: {other:?}"),
        }
    }

    /// Rename, move, or both, through 02.
    pub async fn update(&self, node: Uuid, name: Option<&str>, parent: Option<Uuid>) {
        self.tree
            .update_node(self.account, node, name, parent)
            .await
            .expect("rename or move");
    }

    pub async fn delete(&self, node: Uuid) {
        self.tree
            .delete_node(self.account, node, None)
            .await
            .expect("delete");
    }

    /// 02's tombstone purge at a time past `tombstone_horizon`, until a batch purges nothing.
    pub async fn purge_past_horizon(&self, harness: &Harness) {
        let at = harness
            .clock
            .now_utc()
            .checked_add(harness.tree_config.tombstone_horizon + std::time::Duration::from_secs(1))
            .expect("in range");
        let mut purged = u64::MAX;
        while purged > 0 {
            purged = self
                .maintenance
                .purge_tombstones(at, PURGE_LIMIT)
                .await
                .expect("tombstone purge");
        }
    }

    /// `deleted_at IS NOT NULL` of `node`, read as the schema owner: whether the cascade has marked it.
    pub async fn is_marked(&mut self, node: Uuid) -> bool {
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM nodes WHERE id = $1")
            .bind(node)
            .fetch_one(&mut self.probe)
            .await
            .expect("probe node")
    }

    pub fn probe(&mut self) -> &mut PgConnection {
        &mut self.probe
    }

    async fn committed_blob(&mut self) -> Uuid {
        let blob = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
        )
        .bind(blob)
        .bind(self.account)
        .execute(&mut self.probe)
        .await
        .expect("insert blob");
        blob
    }

    /// The tree service holds pool handles: it goes before the database does.
    pub async fn finish(self) {
        let Owner {
            tree,
            maintenance,
            probe,
            ..
        } = self;
        drop((tree, maintenance));
        probe.close().await.expect("close fixture connection");
    }
}

fn new_revision(revision_id: Uuid, blob_id: Uuid) -> NewRevision {
    let mut hash = [0u8; 32];
    hash[..16].copy_from_slice(revision_id.as_bytes());
    NewRevision {
        revision_id,
        blob_id,
        size: 1,
        content_hash: FileHash::from_bytes(hash),
        blocklist: vec![Hash32::from_bytes([3; 32])],
        media_type: Some("text/plain".to_owned()),
        client_mtime: None,
        client_meta: None,
        reserved_bytes: 0,
        import_hint: false,
    }
}

/// What `authorize_content` must return for `version` of a file of `owner`.
pub fn content_of(owner: &Owner, version: Version) -> ShareContent {
    ShareContent {
        account_id: owner.account,
        node_id: version.node,
        revision_id: version.revision,
    }
}
