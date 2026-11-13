//! `ShareService`: the owner creates, lists and revokes links to his files. The target is resolved through 02's
//! `TreeReader`; every gone case (another account's node, a tombstone, a purged or never existing id) gives the one
//! `TargetNotFound` value, so a create cannot probe node ids of other accounts. Create and the first revoke write
//! their audit event in the link's transaction (fail closed).

use std::sync::Arc;
use std::time::Duration;

use argon2::Params;
use contracts::{
    CreatedShare, LiveFileLookup, NodeKind, ShareError, ShareInfo, ShareService, TreeReader,
    TreeReaderError, async_trait,
};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_clock::{Clock, Timestamp};
use sc_config::KeyProblem;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::{new_id, sha256_token};
use uuid::Uuid;

use crate::config::ShareConfig;
use crate::error::{SharingError, TxError};
use crate::secret::{hash_password, new_share_token};
use crate::share_repo::{self, ListedLink, NewLink};

const SHARE_LINK: &str = "share_link";
/// The public page of a link (16 web app, `/s/<token>`).
const URL_PATH_PREFIX: &str = "/s/";

/// The owner side of share links over `share_links`.
pub struct PgShareService {
    db: Db,
    clock: Arc<dyn Clock>,
    tree: Arc<dyn TreeReader>,
    audit: AuditWriter,
    default_ttl: Duration,
    max_ttl: Duration,
    password_params: Params,
}

impl PgShareService {
    /// Opens no connection. `config` is the validated sharing section; an unusable `share_argon2_params` is
    /// refused here as well, naming the key.
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        tree: Arc<dyn TreeReader>,
        audit: AuditWriter,
        config: &ShareConfig,
    ) -> Result<Self, KeyProblem> {
        Ok(PgShareService {
            db,
            clock,
            tree,
            audit,
            default_ttl: config.default_ttl,
            max_ttl: config.max_ttl,
            password_params: config.password_params()?,
        })
    }

    /// `TargetNotFound` for every gone case, `TargetNotShareable` for a folder.
    async fn check_target(&self, account: Uuid, node_id: Uuid) -> Result<(), ShareError> {
        match self.tree.resolve_live_file(account, node_id).await {
            Ok(LiveFileLookup::Found(file)) if file.kind == NodeKind::File => Ok(()),
            Ok(LiveFileLookup::Found(_)) => Err(ShareError::TargetNotShareable),
            Ok(LiveFileLookup::Gone) | Err(TreeReaderError::NotFound) => {
                Err(ShareError::TargetNotFound)
            }
            Err(other) => Err(ShareError::Unexpected(Box::new(other))),
        }
    }
}

/// The expiry of a new link: `now` plus `expires_in` (default `default_ttl`), which must be above zero and at
/// most `max_ttl`. A negative lifetime cannot reach this type; the boundary (11) maps it to `ExpiryOutOfRange`.
pub(crate) fn expiry(
    now: Timestamp,
    expires_in: Option<Duration>,
    default_ttl: Duration,
    max_ttl: Duration,
) -> Result<Timestamp, ShareError> {
    let ttl = expires_in.unwrap_or(default_ttl);
    if ttl.is_zero() || ttl > max_ttl {
        return Err(ShareError::ExpiryOutOfRange);
    }
    now.checked_add(ttl)
        .ok_or_else(|| unexpected(SharingError::TimeOutOfRange))
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> ShareError {
    ShareError::Unexpected(Box::new(error))
}

fn run_unexpected<E: std::error::Error + Send + Sync + 'static>(error: RunError<E>) -> ShareError {
    match error {
        RunError::Closure(inner) => unexpected(inner),
        RunError::Db(db) => unexpected(db),
    }
}

fn subject(link: Uuid) -> Option<SubjectRef> {
    Some(SubjectRef {
        subject_type: SHARE_LINK,
        id: link,
    })
}

/// Ids only: no token, file name or password.
fn link_event(kind: AuditEventKind, account: Uuid, link: Uuid, node: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(account),
        subject: subject(link),
        detail: AuditDetail::new().with_id("node_id", node),
        ..AuditEvent::new(kind)
    }
}

fn share_info(row: ListedLink) -> ShareInfo {
    ShareInfo {
        id: row.id,
        node_id: row.node_id,
        expires_at: Timestamp::from_unix_micros(row.expires_at_micros),
        has_password: row.has_password,
        created_at: Timestamp::from_unix_micros(row.created_at_micros),
    }
}

#[async_trait]
impl ShareService for PgShareService {
    async fn create(
        &self,
        account: Uuid,
        node_id: Uuid,
        expires_in: Option<Duration>,
        password: Option<&str>,
    ) -> Result<CreatedShare, ShareError> {
        let created_at = self.clock.now_utc();
        let expires_at = expiry(created_at, expires_in, self.default_ttl, self.max_ttl)?;
        self.check_target(account, node_id).await?;
        // Argon2 runs before the transaction opens: no transaction waits on the hash.
        let password_hash = password
            .map(|text| hash_password(&self.password_params, text))
            .transpose()
            .map_err(unexpected)?;
        let token = new_share_token().map_err(unexpected)?;
        let token_hash = sha256_token(token.expose());
        let link = NewLink {
            id: new_id(self.clock.as_ref()),
            account_id: account,
            node_id,
            token_hash: &token_hash,
            password_hash: password_hash.as_deref(),
            expires_at,
            created_at,
        };
        let event = link_event(AuditEventKind::ShareCreated, account, link.id, node_id);
        let stored = self
            .db
            .run(Scope::Account(account), async |tx| {
                share_repo::insert(tx, &link).await?;
                self.audit.record_in(tx, event).await?;
                Ok::<_, TxError>(())
            })
            .await;
        match stored {
            Ok(()) => {}
            // The node was purged between the lookup and the insert: the composite foreign key refuses it.
            Err(RunError::Closure(TxError::Db(DbError::ForeignKeyViolation))) => {
                return Err(ShareError::TargetNotFound);
            }
            Err(other) => return Err(run_unexpected(other)),
        }
        Ok(CreatedShare {
            id: link.id,
            url_path: format!("{URL_PATH_PREFIX}{}", token.expose()),
            token,
            expires_at,
            has_password: password_hash.is_some(),
        })
    }

    async fn list(
        &self,
        account: Uuid,
        node_id: Option<Uuid>,
    ) -> Result<Vec<ShareInfo>, ShareError> {
        let rows = self
            .db
            .run(Scope::Account(account), async |tx| {
                share_repo::list(tx, account, node_id).await
            })
            .await
            .map_err(run_unexpected)?;
        Ok(rows.into_iter().map(share_info).collect())
    }

    async fn revoke(&self, account: Uuid, share_id: Uuid) -> Result<(), ShareError> {
        let now = self.clock.now_utc();
        let found = self
            .db
            .run(Scope::Account(account), async |tx| {
                let Some(node) = share_repo::revoke(tx, account, share_id, now).await? else {
                    return Ok(share_repo::exists(tx, account, share_id).await?);
                };
                let event = link_event(AuditEventKind::ShareRevoked, account, share_id, node);
                self.audit.record_in(tx, event).await?;
                Ok::<_, TxError>(true)
            })
            .await
            .map_err(run_unexpected)?;
        if found {
            Ok(())
        } else {
            Err(ShareError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);
    const DAY: Duration = Duration::from_secs(86_400);
    const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

    fn expiry_of(expires_in: Option<Duration>) -> Result<Timestamp, ShareError> {
        expiry(NOW, expires_in, 7 * DAY, 365 * DAY)
    }

    #[test]
    fn expiry_accepts_above_zero_up_to_the_maximum() {
        // Arrange
        let accepted = [
            (None, 7 * DAY),
            (Some(Duration::from_micros(1)), Duration::from_micros(1)),
            (Some(HOUR), HOUR),
            (Some(365 * DAY), 365 * DAY),
        ];

        // Act
        let results: Vec<_> = accepted
            .iter()
            .map(|(input, _)| expiry_of(*input))
            .collect();

        // Assert
        for ((_, ttl), result) in accepted.iter().zip(results) {
            assert_eq!(
                result.expect("accepted"),
                NOW.checked_add(*ttl).expect("in range")
            );
        }
    }

    #[test]
    fn expiry_refuses_zero_and_above_the_maximum() {
        // Arrange
        let refused = [
            Duration::ZERO,
            365 * DAY + Duration::from_micros(1),
            366 * DAY,
        ];

        // Act
        let results: Vec<_> = refused.iter().map(|ttl| expiry_of(Some(*ttl))).collect();

        // Assert
        for result in results {
            assert!(
                matches!(result, Err(ShareError::ExpiryOutOfRange)),
                "{result:?}"
            );
        }
    }
}
