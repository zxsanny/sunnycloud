//! Recording doubles of the 02 tree ports (AC-1 call surface, AC-4). `TreeCommit` is in `commit_effect.rs`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::{Calls, NOW, id, sorted};
use contracts::{
    ListingPage, LiveFile, LiveFileLookup, LiveUsageError, LiveUsageSource, MaintenanceError,
    NodeKind, NodeOrPath, NodeRef, NodeSelector, NodeState, NodeView, Page, Placement,
    RestoreError, RestoreOutcome, RevisionMatch, RevisionRef, RevisionView, TreeMaintenance,
    TreeReader, TreeReaderError, TreeRestore, TreeService, TreeServiceError, async_trait,
};
use sc_ids::{FileHash, Hash32};
use uuid::Uuid;

#[derive(Default)]
struct Double {
    calls: Calls,
    gone: bool,
}

fn hash() -> FileHash {
    FileHash::from_bytes([3; 32])
}

fn node(kind: NodeKind) -> NodeView {
    NodeView {
        id: id(),
        parent_id: Some(Uuid::from_u128(1)),
        kind,
        name: "a".to_owned(),
        state: NodeState::Live,
        current_revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        client_mtime: None,
        created_at: NOW,
        updated_at: NOW,
        seq: 1,
    }
}

fn revision() -> RevisionView {
    RevisionView {
        id: Uuid::from_u128(9),
        node_id: id(),
        size: 5,
        content_hash: hash(),
        blocklist: Some(vec![Hash32::from_bytes([1; 32])]),
        media_type: Some("text/plain".to_owned()),
        client_mtime: None,
        client_meta: None,
        created_at: NOW,
    }
}

#[async_trait]
impl TreeService for Double {
    async fn create_folder(
        &self,
        _account: Uuid,
        _parent_id: Uuid,
        _name: &str,
    ) -> Result<NodeView, TreeServiceError> {
        self.calls.record("create_folder");
        Ok(node(NodeKind::Folder))
    }

    async fn update_node(
        &self,
        _account: Uuid,
        _node_id: Uuid,
        _new_name: Option<&str>,
        _new_parent: Option<Uuid>,
    ) -> Result<NodeView, TreeServiceError> {
        self.calls.record("update_node");
        Ok(node(NodeKind::File))
    }

    async fn delete_node(
        &self,
        _account: Uuid,
        _node_id: Uuid,
        _base_revision: Option<Uuid>,
    ) -> Result<(), TreeServiceError> {
        self.calls.record("delete_node");
        Ok(())
    }

    async fn resolve_path(&self, _account: Uuid, _path: &str) -> Result<Uuid, TreeServiceError> {
        self.calls.record("resolve_path");
        Ok(id())
    }

    async fn find_revisions(
        &self,
        _account: Uuid,
        _client_device: Option<&str>,
        _page: Page,
    ) -> Result<Vec<RevisionRef>, TreeServiceError> {
        self.calls.record("find_revisions");
        Ok(Vec::new())
    }
}

#[async_trait]
impl TreeReader for Double {
    async fn get_node(
        &self,
        _account: Uuid,
        _selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        self.calls.record("get_node");
        Ok(node(NodeKind::File))
    }

    async fn list_children(
        &self,
        _account: Uuid,
        _folder_id: Uuid,
        _after_name_key: Option<&str>,
        _limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        self.calls.record("list_children");
        Ok(ListingPage {
            items: Vec::new(),
            next: None,
        })
    }

    async fn find_live_node(
        &self,
        _account: Uuid,
        _parent_id: Uuid,
        _name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        self.calls.record("find_live_node");
        Ok(None)
    }

    async fn resolve_live_file(
        &self,
        _account: Uuid,
        _node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        self.calls.record("resolve_live_file");
        if self.gone {
            return Ok(LiveFileLookup::Gone);
        }
        Ok(LiveFileLookup::Found(LiveFile {
            kind: NodeKind::Folder,
            revision: None,
            blob_ref: None,
        }))
    }

    async fn root_node(&self, _account: Uuid) -> Result<NodeRef, TreeReaderError> {
        self.calls.record("root_node");
        Ok(NodeRef {
            id: id(),
            kind: NodeKind::Folder,
        })
    }

    async fn get_revision(
        &self,
        _account: Uuid,
        _revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError> {
        self.calls.record("get_revision");
        Ok(revision())
    }

    async fn is_current_or_retained(
        &self,
        _account: Uuid,
        _revision_id: Uuid,
    ) -> Result<bool, TreeReaderError> {
        self.calls.record("is_current_or_retained");
        Ok(true)
    }

    async fn describe_blobs(
        &self,
        blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError> {
        self.calls.record("describe_blobs");
        Ok(blob_ids
            .iter()
            .map(|blob| (*blob, vec!["/a".to_owned()]))
            .collect())
    }

    async fn lookup_revisions(
        &self,
        _account: Uuid,
        hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError> {
        self.calls.record("lookup_revisions");
        Ok(hashes
            .iter()
            .map(|hash| RevisionMatch {
                hash: *hash,
                node_id: id(),
                revision_id: Uuid::from_u128(9),
                client_meta: None,
            })
            .collect())
    }
}

#[async_trait]
impl TreeRestore for Double {
    async fn restore_revision(
        &self,
        _account: Uuid,
        _target: NodeOrPath,
        _at: sc_clock::Timestamp,
    ) -> Result<RestoreOutcome, RestoreError> {
        self.calls.record("restore_revision");
        Ok(RestoreOutcome {
            restored_revision: Uuid::from_u128(9),
            placement: Placement::Original,
        })
    }
}

#[async_trait]
impl LiveUsageSource for Double {
    async fn live_bytes(&self, _account: Uuid) -> Result<i64, LiveUsageError> {
        self.calls.record("live_bytes");
        Ok(42)
    }
}

#[async_trait]
impl TreeMaintenance for Double {
    async fn cascade_batch(
        &self,
        _now: sc_clock::Timestamp,
        _limit: u32,
    ) -> Result<u64, MaintenanceError> {
        self.calls.record("cascade_batch");
        Ok(1)
    }

    async fn purge_tombstones(
        &self,
        _now: sc_clock::Timestamp,
        _limit: u32,
    ) -> Result<u64, MaintenanceError> {
        self.calls.record("purge_tombstones");
        Ok(2)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tree_service_double_records_each_method_through_arc_dyn() {
    let calls = Calls::default();
    let service: Arc<dyn TreeService> = Arc::new(Double {
        calls: calls.clone(),
        gone: false,
    });
    let account = id();

    let handle = tokio::spawn(async move {
        service.create_folder(account, account, "f").await?;
        service
            .update_node(account, account, Some("g"), None)
            .await?;
        service.delete_node(account, account, None).await?;
        service.resolve_path(account, "/g").await?;
        service
            .find_revisions(
                account,
                None,
                Page {
                    offset: 0,
                    limit: 10,
                },
            )
            .await?;
        Ok::<_, TreeServiceError>(())
    });
    handle.await.expect("task").expect("calls");

    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "create_folder",
            "update_node",
            "delete_node",
            "resolve_path",
            "find_revisions"
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tree_reader_double_records_each_method_through_arc_dyn() {
    let calls = Calls::default();
    let reader: Arc<dyn TreeReader> = Arc::new(Double {
        calls: calls.clone(),
        gone: false,
    });
    let account = id();

    let handle = tokio::spawn(async move {
        reader.get_node(account, NodeSelector::Root).await?;
        reader.list_children(account, account, None, 1000).await?;
        reader.find_live_node(account, account, "a").await?;
        reader.resolve_live_file(account, account).await?;
        reader.root_node(account).await?;
        reader.get_revision(account, account).await?;
        reader.is_current_or_retained(account, account).await?;
        let described = reader.describe_blobs(&[account]).await?;
        let matches = reader.lookup_revisions(account, &[hash()]).await?;
        Ok::<_, TreeReaderError>((described.len(), matches.len()))
    });
    let (described, matches) = handle.await.expect("task").expect("calls");

    assert_eq!((described, matches), (1, 1));
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "get_node",
            "list_children",
            "find_live_node",
            "resolve_live_file",
            "root_node",
            "get_revision",
            "is_current_or_retained",
            "describe_blobs",
            "lookup_revisions"
        ])
    );
}

#[tokio::test]
async fn restore_usage_and_maintenance_doubles_record_each_method() {
    let calls = Calls::default();
    let double = Arc::new(Double {
        calls: calls.clone(),
        gone: false,
    });
    let restore: Arc<dyn TreeRestore> = double.clone();
    let usage: Arc<dyn LiveUsageSource> = double.clone();
    let maintenance: Arc<dyn TreeMaintenance> = double;

    let outcome = restore
        .restore_revision(id(), NodeOrPath::Path("/a".to_owned()), NOW)
        .await
        .expect("restore");
    let bytes = usage.live_bytes(id()).await.expect("live_bytes");
    let cascaded = maintenance.cascade_batch(NOW, 10).await.expect("cascade");
    let purged = maintenance.purge_tombstones(NOW, 10).await.expect("purge");

    assert_eq!(outcome.placement, Placement::Original);
    assert_eq!((bytes, cascaded, purged), (42, 1, 2));
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "restore_revision",
            "live_bytes",
            "cascade_batch",
            "purge_tombstones"
        ])
    );
}

/// What a caller of `resolve_live_file` does: the three cases are told apart by value, never by an error.
fn classify(lookup: &LiveFileLookup) -> &'static str {
    match lookup {
        LiveFileLookup::Gone => "gone",
        LiveFileLookup::Found(LiveFile {
            kind: NodeKind::Folder,
            ..
        }) => "folder",
        LiveFileLookup::Found(LiveFile {
            kind: NodeKind::File,
            ..
        }) => "file",
    }
}

#[tokio::test]
async fn a_folder_is_a_found_live_file_with_no_revision_and_a_gone_node_is_a_separate_value() {
    let folder = Double::default();
    let gone = Double {
        calls: Calls::default(),
        gone: true,
    };

    let found = folder
        .resolve_live_file(id(), id())
        .await
        .expect("folder lookup is Ok");
    let missing = gone
        .resolve_live_file(id(), id())
        .await
        .expect("gone lookup is Ok, not an error");

    assert_eq!(classify(&found), "folder");
    assert_eq!(classify(&missing), "gone");
    let LiveFileLookup::Found(live) = found else {
        panic!("a folder is found");
    };
    assert_eq!((live.revision, live.blob_ref), (None, None));
}

#[test]
fn a_file_lookup_carries_the_revision_and_the_blob_id() {
    let live = LiveFile {
        kind: NodeKind::File,
        revision: Some(revision()),
        blob_ref: Some(Uuid::from_u128(5)),
    };

    assert_eq!(classify(&LiveFileLookup::Found(live)), "file");
}
