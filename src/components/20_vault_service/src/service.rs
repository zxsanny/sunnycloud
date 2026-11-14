//! `VaultService`: create, read and replace the opaque vault header. The header is never parsed; its ETag is the
//! SHA-256 of the stored bytes, computed by the database over the stored column (`crate::vault_repo`).
//!
//! The root is checked through 02's `TreeReader`: the account's own root folder is `NotFound` (owner decision
//! 2026-11-13: every file the account creates would sit in the vault), then one `list_children` call of limit 1
//! makes an unknown, foreign or tombstoned node (also under a tombstoned ancestor) and a file `NotFound` and a folder
//! with a live child `VaultExists`, so a vault starts on an empty folder. One vault per account and one vault per
//! root are kept by the unique keys of `vaults`.
//!
//! An unlock-failure report (E-30/c04) writes one `vault_failure` audit event, ids only, through
//! `AuditWriter::record` (its own short transaction, fail closed) after a separate lookup of the vault in the
//! caller's account; an unknown or foreign vault writes nothing and is `NotFound`.

use std::sync::Arc;

use contracts::{
    CreatedVault, ETag, TreeReader, TreeReaderError, VaultError, VaultHeader, VaultService,
    async_trait,
};
use sc_audit::{AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_clock::Clock;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::new_id;
use uuid::Uuid;

use crate::config::VaultConfig;
use crate::vault_repo::{self, NewVault};

/// `audit_log.subject_type` of a vault.
const VAULT_SUBJECT: &str = "vault";

/// The header service over `vaults`.
pub struct PgVaultService {
    db: Db,
    tree: Arc<dyn TreeReader>,
    clock: Arc<dyn Clock>,
    audit: AuditWriter,
    max_header_bytes: u64,
}

impl PgVaultService {
    /// Opens no connection. `config` is the validated vault section.
    pub fn new(
        db: Db,
        tree: Arc<dyn TreeReader>,
        clock: Arc<dyn Clock>,
        audit: AuditWriter,
        config: &VaultConfig,
    ) -> Self {
        PgVaultService {
            db,
            tree,
            clock,
            audit,
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

/// The `vault_failure` event of a report: the account as actor, the vault as subject, no detail. The time is
/// stamped by the writer.
fn unlock_failure_event(account: Uuid, vault_id: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(account),
        subject: Some(SubjectRef {
            subject_type: VAULT_SUBJECT,
            id: vault_id,
        }),
        ..AuditEvent::new(AuditEventKind::VaultFailure)
    }
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

    async fn report_unlock_failure(&self, account: Uuid, vault_id: Uuid) -> Result<(), VaultError> {
        // The lookup commits before the audit write: no transaction spans both.
        let owned = self
            .db
            .run(Scope::Account(account), async |tx| {
                vault_repo::exists(tx, account, vault_id).await
            })
            .await
            .map_err(run_unexpected)?;
        if !owned {
            return Err(VaultError::NotFound);
        }
        self.audit
            .record(unlock_failure_event(account, vault_id))
            .await
            .map_err(unexpected)
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

    #[test]
    fn unlock_failure_event_names_the_account_and_the_vault_and_nothing_else() {
        // Arrange
        let account = Uuid::from_u128(1);
        let vault = Uuid::from_u128(2);

        // Act
        let event = unlock_failure_event(account, vault);

        // Assert
        assert_eq!(
            event,
            AuditEvent {
                event: AuditEventKind::VaultFailure,
                actor_account_id: Some(account),
                actor_session_id: None,
                subject: Some(SubjectRef {
                    subject_type: "vault",
                    id: vault,
                }),
                ip: None,
                detail: sc_audit::AuditDetail::new(),
            }
        );
    }
}
