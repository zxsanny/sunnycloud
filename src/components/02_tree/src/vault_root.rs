//! `VaultRoots::claim_root` (E-30/c01, AZ-3927): 02's half of vault creation, run inside 20's transaction after
//! 20 inserted the `vaults` row. The folder row is locked `FOR UPDATE`, which waits for every create or move that
//! holds it `FOR SHARE` as its parent (`node_repo::lock_for_share`); the emptiness check is a later statement, so it
//! sees every child those committed. A child added after this lock reads the parent's `vault_id` under its own
//! lock once this transaction commits, and copies the new vault. No feed stamp: membership is not a field of the
//! changes feed.

use contracts::{NodeKind, VaultRootError, VaultRoots, async_trait};
use sc_db::{DbError, DbTx};
use uuid::Uuid;

use crate::service::PgTreeService;
use crate::{conflict_repo, node_repo};

#[async_trait]
impl VaultRoots for PgTreeService {
    async fn claim_root(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        root_node_id: Uuid,
        vault: Uuid,
    ) -> Result<(), VaultRootError> {
        let folder = node_repo::lock_for_update(tx, account, root_node_id)
            .await
            .map_err(unexpected)?
            .filter(|node| node.kind == NodeKind::Folder && !node.deleted)
            .ok_or(VaultRootError::NotFound)?;
        // The account's own root folder is refused even when empty (owner decision 2026-11-13).
        let parent = folder.parent_id.ok_or(VaultRootError::NotFound)?;
        if !conflict_repo::tombstoned_ancestors(tx, account, parent)
            .await
            .map_err(unexpected)?
            .is_empty()
        {
            return Err(VaultRootError::NotFound);
        }
        if folder.vault_id.is_some()
            || node_repo::has_live_child(tx, account, root_node_id)
                .await
                .map_err(unexpected)?
        {
            return Err(VaultRootError::Taken);
        }
        node_repo::set_vault(tx, account, root_node_id, vault)
            .await
            .map_err(unexpected)
    }
}

fn unexpected(error: DbError) -> VaultRootError {
    VaultRootError::Unexpected(Box::new(error))
}
