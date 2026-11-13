//! The tree reads of `start_upload`, all before its transaction and none of them a write: the parent check and
//! the tree cases of the `NothingToSend` rule (`upload_service.md` § `NothingToSend` rule). The committed-key
//! case of that rule reads `upload_sessions` and runs first, in `crate::upload`.

use contracts::{
    LiveFileLookup, NodeKind, NodeSelector, NodeState, TreeReader, TreeReaderError, UploadError,
};
use sc_ids::FileHash;
use sc_names::ValidName;
use uuid::Uuid;

/// A live folder of the account. A tombstoned parent, or one under a tombstoned ancestor, is not found: the
/// port has no `ParentDeleted`.
pub(crate) async fn check_parent(
    tree: &dyn TreeReader,
    account: Uuid,
    parent_id: Uuid,
) -> Result<(), UploadError> {
    match tree.get_node(account, NodeSelector::Id(parent_id)).await {
        Ok(node) if node.state == NodeState::Tombstone => Err(UploadError::ParentNotFound),
        Ok(node) if node.kind != NodeKind::Folder => Err(UploadError::NotAFolder),
        Ok(_) => Ok(()),
        Err(TreeReaderError::NotFound) => Err(UploadError::ParentNotFound),
        Err(other) => Err(tree_fault(other)),
    }
}

/// The current revision that already holds `content_hash`, if any: of the node owning `base_revision` when one
/// is given, else of the live file at `(parent_id, name)` matched by `name_key`. A folder, a tombstone, another
/// hash, no node, and an unknown or foreign base all give `None` (a session starts).
pub(crate) async fn same_content(
    tree: &dyn TreeReader,
    account: Uuid,
    parent_id: Uuid,
    name: &ValidName,
    base_revision: Option<Uuid>,
    content_hash: &FileHash,
) -> Result<Option<Uuid>, UploadError> {
    let node = match base_revision {
        Some(base) => match tree.get_revision(account, base).await {
            Ok(revision) => revision.node_id,
            Err(TreeReaderError::NotFound) => return Ok(None),
            Err(other) => return Err(tree_fault(other)),
        },
        None => match tree.find_live_node(account, parent_id, name.as_str()).await {
            Ok(Some(node)) if node.kind == NodeKind::File => node.id,
            Ok(_) => return Ok(None),
            Err(other) => return Err(tree_fault(other)),
        },
    };
    let file = match tree.resolve_live_file(account, node).await {
        Ok(LiveFileLookup::Found(file)) if file.kind == NodeKind::File => file,
        Ok(_) | Err(TreeReaderError::NotFound) => return Ok(None),
        Err(other) => return Err(tree_fault(other)),
    };
    Ok(file
        .revision
        .filter(|revision| revision.content_hash == *content_hash)
        .map(|revision| revision.id))
}

fn tree_fault(error: TreeReaderError) -> UploadError {
    UploadError::Unexpected(Box::new(error))
}
