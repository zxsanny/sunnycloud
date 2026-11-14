//! `VaultRoots`: 02's half of vault creation (v1.2, E-30/c01). 20 calls it inside its own transaction, after it
//! inserted the `vaults` row, so the root's membership and the vault commit together or not at all.

use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

#[derive(Debug, Error)]
pub enum VaultRootError {
    /// Unknown, foreign or tombstoned (also under a tombstoned folder), a file, or the account's own root folder.
    #[error("not found")]
    NotFound,
    /// The folder has a live child or is already in a vault.
    #[error("root taken")]
    Taken,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait VaultRoots: Send + Sync {
    /// Locks the folder row (`FOR UPDATE`), checks it is a live, empty folder of `account` other than its root
    /// folder and not yet in a vault, and sets its membership to `vault`. Joins the caller's transaction. The
    /// lock conflicts with the parent lock every create and move takes, so a child is either seen by the
    /// emptiness check or created after the commit, copying the membership.
    async fn claim_root(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        root_node_id: Uuid,
        vault: Uuid,
    ) -> Result<(), VaultRootError>;
}
