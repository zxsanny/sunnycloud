//! The reads of `TreeReader` that 03, 06, 07, 08, 11, 12 and 20 rely on: a node, a listing page, a live child by
//! name, a live file and the root. Each call is one short account-scoped transaction and nothing is cached, so a
//! delete is visible on the next call. A node is gone when it or any folder above it is tombstoned: the parent
//! chain is walked on every lookup (bounded by the tree depth), so nothing waits for the background cascade.

use std::sync::Arc;

use contracts::{
    ListingPage, LiveFile, LiveFileLookup, NodeKind, NodeRef, NodeSelector, NodeState, NodeView,
    ThumbnailStateSource, TreeReaderError,
};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sc_names::{MAX_NAME_BYTES, name_key, validate_name};
use uuid::Uuid;

use crate::config::TreeConfig;
use crate::{conflict_repo, node_repo, reader_repo};

/// The largest stored `name_key` (`nodes_name_key_size_check`): a fold can make a key longer than its name.
const MAX_KEY_BYTES: usize = 1024;

/// Separate from `PgTreeService` because it reads the listing limits of `TreeConfig` and needs none of the
/// mutation ports. `TreeReader` and `LiveUsageSource` are implemented in `crate::reader_revisions`, which adds the
/// revision reads; the node reads stay inherent here too.
pub struct PgTreeReader {
    pub(crate) db: Db,
    default_limit: u64,
    max_limit: u64,
    /// Set by `with_thumbnails` (`crate::reader_thumbs`); without it every listing item is `ThumbState::None`.
    pub(crate) thumbnails: Option<Arc<dyn ThumbnailStateSource>>,
}

impl PgTreeReader {
    pub fn new(db: Db, config: &TreeConfig) -> Self {
        PgTreeReader {
            db,
            default_limit: config.listing_default_limit,
            max_limit: config.listing_max_limit,
            thumbnails: None,
        }
    }

    /// A node under a tombstoned ancestor is reported as a tombstone.
    pub async fn get_node(
        &self,
        account: Uuid,
        selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        self.read(account, async |tx| {
            let id = match selector {
                NodeSelector::Id(id) => id,
                NodeSelector::Root => root(tx, account).await?,
            };
            let mut view = node_repo::view(tx, account, id)
                .await
                .map_err(unexpected)?
                .ok_or(TreeReaderError::NotFound)?;
            if view.state == NodeState::Live && gone_above(tx, account, view.parent_id).await? {
                view.state = NodeState::Tombstone;
            }
            Ok(view)
        })
        .await
    }

    /// Live children in `name_key` order after the cursor. A limit of 0 asks for `listing_default_limit`; a limit
    /// above `listing_max_limit` is clamped to it (default 1000, owner decision 2026-11-13). A deleted folder is `NotFound`.
    /// The page's image and video items carry their thumbnail state, read after the page's transaction.
    pub async fn list_children(
        &self,
        account: Uuid,
        folder_id: Uuid,
        after_name_key: Option<&str>,
        limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        if after_name_key.is_some_and(|cursor| !is_name_key(cursor)) {
            return Err(TreeReaderError::InvalidCursor);
        }
        let size = page_size(self.default_limit, self.max_limit, limit);
        let fetch = i64::try_from(size.saturating_add(1)).unwrap_or(i64::MAX);
        let mut page = self
            .read(account, async |tx| {
                let folder = node_repo::view(tx, account, folder_id)
                    .await
                    .map_err(unexpected)?
                    .filter(|view| view.state == NodeState::Live)
                    .ok_or(TreeReaderError::NotFound)?;
                if gone_above(tx, account, folder.parent_id).await? {
                    return Err(TreeReaderError::NotFound);
                }
                if folder.kind != NodeKind::Folder {
                    return Err(TreeReaderError::NotAFolder);
                }
                let mut rows = reader_repo::page(tx, account, folder_id, after_name_key, fetch)
                    .await
                    .map_err(unexpected)?;
                let more = u64::try_from(rows.len()).is_ok_and(|fetched| fetched > size);
                if more {
                    rows.pop();
                }
                let next = rows.last().filter(|_| more).map(|row| row.name_key.clone());
                Ok(ListingPage {
                    items: rows.into_iter().map(|row| row.view).collect(),
                    next,
                })
            })
            .await?;
        self.fill_thumb_states(account, &mut page.items).await?;
        Ok(page)
    }

    /// The live child of a live folder whose `name_key` is the fold of `name`; an invalid name matches nothing.
    pub async fn find_live_node(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        let Ok(name) = validate_name(name) else {
            return Ok(None);
        };
        let key = name_key(&name);
        self.read(account, async |tx| {
            let child = reader_repo::live_child(tx, account, parent_id, key.as_str())
                .await
                .map_err(unexpected)?;
            match child {
                Some(_) if gone_above(tx, account, Some(parent_id)).await? => Ok(None),
                Some((id, kind)) => Ok(Some(NodeRef { id, kind })),
                None => Ok(None),
            }
        })
        .await
    }

    /// `Gone` for a tombstoned, purged or foreign node and for a node under a tombstoned folder.
    pub async fn resolve_live_file(
        &self,
        account: Uuid,
        node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        self.read(account, async |tx| {
            let Some(row) = reader_repo::live_file(tx, account, node_id)
                .await
                .map_err(unexpected)?
            else {
                return Ok(LiveFileLookup::Gone);
            };
            if row.deleted || gone_above(tx, account, row.parent_id).await? {
                return Ok(LiveFileLookup::Gone);
            }
            let found = match (row.kind, row.revision) {
                (NodeKind::Folder, _) => LiveFile {
                    kind: NodeKind::Folder,
                    revision: None,
                    blob_ref: None,
                },
                (NodeKind::File, Some((revision, blob_id))) => LiveFile {
                    kind: NodeKind::File,
                    revision: Some(revision),
                    blob_ref: Some(blob_id),
                },
                // The pointer is cleared only on a tombstone whose revision was collected.
                (NodeKind::File, None) => {
                    return Err(unexpected(DbError::Other(format!(
                        "live file {node_id} has no current revision"
                    ))));
                }
            };
            Ok(LiveFileLookup::Found(found))
        })
        .await
    }

    /// `NotFound` only for an account whose root is gone (erased).
    pub async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError> {
        self.read(account, async |tx| {
            Ok(NodeRef {
                id: root(tx, account).await?,
                kind: NodeKind::Folder,
            })
        })
        .await
    }

    pub(crate) async fn read<T>(
        &self,
        account: Uuid,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, TreeReaderError>,
    ) -> Result<T, TreeReaderError> {
        self.db
            .run(Scope::Account(account), f)
            .await
            .map_err(|error| match error {
                RunError::Closure(error) => error,
                RunError::Db(error) => unexpected(error),
            })
    }
}

fn page_size(default_limit: u64, max_limit: u64, requested: u32) -> u64 {
    let wanted = match requested {
        0 => default_limit,
        requested => u64::from(requested),
    };
    wanted.min(max_limit)
}

async fn root(tx: &mut DbTx, account: Uuid) -> Result<Uuid, TreeReaderError> {
    reader_repo::root(tx, account)
        .await
        .map_err(unexpected)?
        .ok_or(TreeReaderError::NotFound)
}

/// Whether `parent` or a folder above it is tombstoned; the root has no parent.
async fn gone_above(
    tx: &mut DbTx,
    account: Uuid,
    parent: Option<Uuid>,
) -> Result<bool, TreeReaderError> {
    let Some(parent) = parent else {
        return Ok(false);
    };
    let tombstoned = conflict_repo::tombstoned_ancestors(tx, account, parent)
        .await
        .map_err(unexpected)?;
    Ok(!tombstoned.is_empty())
}

/// Whether `cursor` can be a stored `name_key`. A key of up to 255 bytes is the fold of a valid name and its own
/// fold; a longer one (a fold that grew the name) is checked for the characters no name holds.
fn is_name_key(cursor: &str) -> bool {
    if cursor.len() <= MAX_NAME_BYTES {
        return validate_name(cursor).is_ok_and(|name| name_key(&name).as_str() == cursor);
    }
    cursor.len() <= MAX_KEY_BYTES && !cursor.chars().any(|c| c == '/' || c.is_control())
}

pub(crate) fn unexpected(error: DbError) -> TreeReaderError {
    TreeReaderError::Unexpected(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_above_the_maximum_is_clamped_and_zero_asks_for_the_default() {
        // Arrange
        let (default_limit, max_limit) = (1000, 2000);

        // Act
        let sizes = [
            page_size(default_limit, max_limit, 5000),
            page_size(default_limit, max_limit, 0),
            page_size(default_limit, max_limit, 7),
            page_size(default_limit, max_limit, u32::MAX),
        ];

        // Assert
        assert_eq!(sizes, [2000, 1000, 7, 2000]);
    }

    #[test]
    fn a_garbage_cursor_is_refused_and_a_stored_key_is_accepted() {
        // Arrange
        let long_key = "ss".repeat(200);
        let garbage = [
            "",
            "a/b",
            "nul\0",
            "..",
            "Upper",
            "bidi\u{202E}",
            &"x".repeat(MAX_KEY_BYTES + 1),
        ];
        let stored = name_key(&validate_name("Straße Ünïcode.TXT").expect("valid name"));

        // Act
        let refused: Vec<bool> = garbage.iter().map(|cursor| is_name_key(cursor)).collect();
        let accepted = [is_name_key(stored.as_str()), is_name_key(&long_key)];

        // Assert
        assert_eq!(refused, [false; 7]);
        assert_eq!(accepted, [true, true]);
    }
}
