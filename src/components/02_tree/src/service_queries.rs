//! The path and device queries of `TreeService` and the `TreeService` port itself (the mutations are the inherent
//! methods of `crate::service`). Each query is one short account-scoped read transaction.

use contracts::{
    NodeKind, NodeState, NodeView, Page, RevisionRef, TreeService, TreeServiceError, async_trait,
};
use sc_db::{DbError, DbTx, Scope};
use sc_names::{name_key, split_path};
use uuid::Uuid;

use crate::retry::run_retrying_each;
use crate::service::PgTreeService;
use crate::{reader_repo, revision_repo};

/// The largest page of `find_revisions`, and the page size when the caller asks for 0.
pub const MAX_REVISION_PAGE: u32 = 1000;

impl PgTreeService {
    /// The node at `path` below the root, with its state. Every folder on the way is live; the last part is the
    /// live node of that name, else its most recently tombstoned one (owner decision 2026-11-13: restore of a
    /// deleted file uses it). A path that is not a sequence of valid names (`..`, `.`, empty parts) is `NotFound`.
    pub async fn resolve_path_with_state(
        &self,
        account: Uuid,
        path: &str,
    ) -> Result<(Uuid, NodeState), TreeServiceError> {
        let Ok(names) = split_path(path) else {
            return Err(TreeServiceError::NotFound);
        };
        let keys: Vec<_> = names.iter().map(name_key).collect();
        let Some((last, folders)) = keys.split_last() else {
            return Err(TreeServiceError::NotFound);
        };
        let (last, folders) = (last.as_str(), folders);
        run_retrying_each(&self.db, Scope::Account(account), || {
            async move |tx: &mut DbTx| {
                let mut parent = reader_repo::root(tx, account)
                    .await
                    .map_err(unexpected)?
                    .ok_or(TreeServiceError::NotFound)?;
                for key in folders {
                    parent = live_folder(tx, account, parent, key.as_str()).await?;
                }
                let (id, deleted) = reader_repo::child_by_key(tx, account, parent, last)
                    .await
                    .map_err(unexpected)?
                    .ok_or(TreeServiceError::NotFound)?;
                let state = if deleted {
                    NodeState::Tombstone
                } else {
                    NodeState::Live
                };
                Ok((id, state))
            }
        })
        .await
    }
}

#[async_trait]
impl TreeService for PgTreeService {
    async fn create_folder(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<NodeView, TreeServiceError> {
        PgTreeService::create_folder(self, account, parent_id, name).await
    }

    async fn update_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_parent: Option<Uuid>,
    ) -> Result<NodeView, TreeServiceError> {
        PgTreeService::update_node(self, account, node_id, new_name, new_parent).await
    }

    async fn delete_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        base_revision: Option<Uuid>,
    ) -> Result<(), TreeServiceError> {
        PgTreeService::delete_node(self, account, node_id, base_revision).await
    }

    /// A live node only: a tombstoned last part is `NotFound` here (`resolve_path_with_state` returns it).
    async fn resolve_path(&self, account: Uuid, path: &str) -> Result<Uuid, TreeServiceError> {
        match self.resolve_path_with_state(account, path).await? {
            (id, NodeState::Live) => Ok(id),
            (_, NodeState::Tombstone) => Err(TreeServiceError::NotFound),
        }
    }

    /// Offset paging in revision id order (the `Page` of the port). A limit of 0 asks for `MAX_REVISION_PAGE`,
    /// a larger one is cut to it; an offset beyond what the database can address is `InvalidCursor`.
    async fn find_revisions(
        &self,
        account: Uuid,
        client_device: Option<&str>,
        page: Page,
    ) -> Result<Vec<RevisionRef>, TreeServiceError> {
        let offset = i64::try_from(page.offset).map_err(|_| TreeServiceError::InvalidCursor)?;
        let limit = match page.limit {
            0 => MAX_REVISION_PAGE,
            limit => limit.min(MAX_REVISION_PAGE),
        };
        run_retrying_each(&self.db, Scope::Account(account), || {
            async move |tx: &mut DbTx| {
                revision_repo::page(tx, account, client_device, offset, i64::from(limit))
                    .await
                    .map_err(unexpected)
            }
        })
        .await
    }
}

/// The live folder of `parent` holding `key`; a file or a missing name ends the path as `NotFound`.
async fn live_folder(
    tx: &mut DbTx,
    account: Uuid,
    parent: Uuid,
    key: &str,
) -> Result<Uuid, TreeServiceError> {
    match reader_repo::live_child(tx, account, parent, key)
        .await
        .map_err(unexpected)?
    {
        Some((id, NodeKind::Folder)) => Ok(id),
        Some((_, NodeKind::File)) | None => Err(TreeServiceError::NotFound),
    }
}

fn unexpected(error: DbError) -> TreeServiceError {
    TreeServiceError::Unexpected(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_an_empty_or_dot_part_is_refused_before_any_query() {
        // Arrange
        let refused = ["", "../x", "a//b", "a/./b", "/a", "a/", ".", ".."];

        // Act
        let split: Vec<bool> = refused
            .iter()
            .map(|path| split_path(path).is_err())
            .collect();
        let accepted = split_path("a/b.txt").map(|names| names.len());

        // Assert
        assert_eq!(split, [true; 8]);
        assert_eq!(accepted, Ok(2));
    }
}
