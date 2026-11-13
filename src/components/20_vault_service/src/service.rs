//! `VaultService`: create, read and replace the opaque vault header. The header is never parsed; its ETag is the
//! SHA-256 of the stored bytes, computed by the database over the stored column (`crate::vault_repo`).
//!
//! The root is checked through 02's `TreeReader`: the account's own root folder is `NotFound` (owner decision
//! 2026-11-13: every file the account creates would sit in the vault), then one `list_children` call of limit 1
//! makes an unknown, foreign or tombstoned node (also under a tombstoned ancestor) and a file `NotFound` and a folder
//! with a live child `VaultExists`, so a vault starts on an empty folder. One vault per account and one vault per
//! root are kept by the unique keys of `vaults`.

use std::sync::Arc;

use contracts::{
    CreatedVault, ETag, TreeReader, TreeReaderError, VaultError, VaultHeader, VaultService,
    async_trait,
};
use sc_clock::Clock;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::new_id;
use uuid::Uuid;

use crate::config::VaultConfig;
use crate::vault_repo::{self, NewVault};

/// Out of scope of the header service (E-30/c04, its own task); the port answers this until it is built.
const UNLOCK_FAILURE_NOT_BUILT: &str = "report_unlock_failure is not implemented yet (E-30/c04)";

/// The header service over `vaults`.
pub struct PgVaultService {
    db: Db,
    tree: Arc<dyn TreeReader>,
    clock: Arc<dyn Clock>,
    max_header_bytes: u64,
}

impl PgVaultService {
    /// Opens no connection. `config` is the validated vault section.
    pub fn new(
        db: Db,
        tree: Arc<dyn TreeReader>,
        clock: Arc<dyn Clock>,
        config: &VaultConfig,
    ) -> Self {
        PgVaultService {
            db,
            tree,
            clock,
            max_header_bytes: config.max_vault_header_bytes,
        }
    }

    fn check_size(&self, header: &[u8]) -> Result<(), VaultError> {
        if within_cap(header.len(), self.max_header_bytes) {
            Ok(())
        } else {
            Err(VaultError::HeaderTooLarge)
        }
    }

    /// A live, empty folder of `account` other than its own root folder.
    async fn check_root(&self, account: Uuid, root_node_id: Uuid) -> Result<(), VaultError> {
        match self.tree.root_node(account).await {
            Ok(root) if root.id == root_node_id => return Err(VaultError::NotFound),
            Ok(_) => {}
            Err(TreeReaderError::NotFound) => return Err(VaultError::NotFound),
            Err(other) => return Err(unexpected(other)),
        }
        match self
            .tree
            .list_children(account, root_node_id, None, 1)
            .await
        {
            Ok(page) if page.items.is_empty() => Ok(()),
            Ok(_) => Err(VaultError::VaultExists),
            Err(TreeReaderError::NotFound | TreeReaderError::NotAFolder) => {
                Err(VaultError::NotFound)
            }
            Err(other) => Err(unexpected(other)),
        }
    }
}

/// `true` when a header of `len` bytes is at most `max` bytes.
pub(crate) fn within_cap(len: usize, max: u64) -> bool {
    u64::try_from(len).is_ok_and(|len| len <= max)
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> VaultError {
    VaultError::Unexpected(Box::new(error))
}

fn run_unexpected(error: RunError<DbError>) -> VaultError {
    match error {
        RunError::Closure(error) | RunError::Db(error) => unexpected(error),
    }
}

#[async_trait]
impl VaultService for PgVaultService {
    async fn create_vault(
        &self,
        account: Uuid,
        header: &[u8],
        root_node_id: Uuid,
    ) -> Result<CreatedVault, VaultError> {
        self.check_size(header)?;
        self.check_root(account, root_node_id).await?;
        let vault = NewVault {
            id: new_id(self.clock.as_ref()),
            account_id: account,
            root_node_id,
            header,
            created_at: self.clock.now_utc(),
        };
        let stored = self
            .db
            .run(Scope::Account(account), async |tx| {
                vault_repo::insert(tx, &vault).await
            })
            .await;
        match stored {
            Ok(Some(etag)) => Ok(CreatedVault {
                vault_id: vault.id,
                etag: ETag(etag),
            }),
            Ok(None) => Err(VaultError::VaultExists),
            // The root was purged between the check and the insert: the composite foreign key refuses it.
            Err(RunError::Closure(DbError::ForeignKeyViolation)) => Err(VaultError::NotFound),
            Err(other) => Err(run_unexpected(other)),
        }
    }

    async fn get_header(&self, account: Uuid, vault_id: Uuid) -> Result<VaultHeader, VaultError> {
        let stored = self
            .db
            .run(Scope::Account(account), async |tx| {
                vault_repo::get(tx, account, vault_id).await
            })
            .await
            .map_err(run_unexpected)?
            .ok_or(VaultError::NotFound)?;
        Ok(VaultHeader {
            header: stored.header,
            etag: ETag(stored.etag),
            root_node_id: stored.root_node_id,
        })
    }

    async fn replace_header(
        &self,
        account: Uuid,
        vault_id: Uuid,
        header: &[u8],
        if_match: &ETag,
    ) -> Result<ETag, VaultError> {
        self.check_size(header)?;
        let now = self.clock.now_utc();
        // The UPDATE alone decides; the existence probe after a miss only tells `NotFound` from a stale ETag.
        self.db
            .run(Scope::Account(account), async |tx| {
                let replaced =
                    vault_repo::replace(tx, account, vault_id, header, &if_match.0, now).await?;
                let outcome = match replaced {
                    Some(etag) => Ok(ETag(etag)),
                    None if vault_repo::exists(tx, account, vault_id).await? => {
                        Err(VaultError::PreconditionFailed)
                    }
                    None => Err(VaultError::NotFound),
                };
                Ok::<_, DbError>(outcome)
            })
            .await
            .map_err(run_unexpected)?
    }

    async fn report_unlock_failure(
        &self,
        _account: Uuid,
        _vault_id: Uuid,
    ) -> Result<(), VaultError> {
        Err(VaultError::Unexpected(UNLOCK_FAILURE_NOT_BUILT.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_cap_accepts_up_to_the_cap_and_refuses_one_more() {
        // Arrange
        let cap = 1024;

        // Act
        let at_cap = within_cap(1024, cap);
        let above = within_cap(1025, cap);

        // Assert
        assert!(at_cap);
        assert!(!above);
    }

    #[test]
    fn within_cap_accepts_the_empty_header() {
        // Arrange
        let cap = 1;

        // Act
        let empty = within_cap(0, cap);

        // Assert
        assert!(empty);
    }
}
