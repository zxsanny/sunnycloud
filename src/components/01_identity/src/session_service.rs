use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Principal, RevokeReason, SessionError, SessionInfo, SessionService, SessionTokens, async_trait,
};
use sc_audit::AuditWriter;
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, DbError, Scope};
use sc_ids::{RawToken, TokenHash, TokenKind, ct_eq, new_token, sha256_token};
use uuid::Uuid;

use crate::audit_events::{self, TxError, record_logged};
use crate::auth_repo;
use crate::config::IdentityConfig;
use crate::error::IdentityError;
use crate::session_repo::{self, LockedSession, Rotation};
use crate::successor::{self, Pair};

const ACTIVE: &str = "active";

/// Refresh rotation, revocation, listing and `last_seen_at` of sessions.
///
/// A refresh finds the session by the presented hash in system scope, then takes that session's row lock in
/// its account's scope and decides under the lock; no other session is locked and nothing but the database
/// is awaited while the lock is held.
///
/// A revocation writes its mandatory `session_revoked` event in its own transaction (a failed write fails the
/// call and nothing is revoked); a refresh reuse is logged at WARN and audited after the revocation commits.
pub struct IdentitySessions {
    db: Db,
    clock: Arc<dyn Clock>,
    audit: AuditWriter,
    access_token_ttl: Duration,
    refresh_token_ttl: Duration,
    refresh_reuse_grace: Duration,
    last_seen_interval: Duration,
}

impl IdentitySessions {
    pub fn new(db: Db, clock: Arc<dyn Clock>, config: &IdentityConfig, audit: AuditWriter) -> Self {
        IdentitySessions {
            db,
            clock,
            audit,
            access_token_ttl: config.access_token_ttl,
            refresh_token_ttl: config.refresh_token_ttl,
            refresh_reuse_grace: config.refresh_reuse_grace,
            last_seen_interval: config.session_last_seen_interval,
        }
    }
}

/// What a presented refresh token means for the locked session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// The current token of a live session: install the successor.
    Rotate,
    /// The previous token inside the grace window: nothing is created and nothing is revoked.
    WithinGrace,
    /// The previous token after the grace window: the session is revoked.
    Reuse,
    /// Unknown, older generation, expired, revoked or of an inactive account.
    Reject,
}

fn decide(row: &LockedSession, presented: &TokenHash, now: Timestamp, grace: Duration) -> Decision {
    if row.revoked
        || row.account_state != ACTIVE
        || row.refresh_expires_at_micros <= now.as_unix_micros()
    {
        return Decision::Reject;
    }
    if ct_eq(&row.refresh_hash, presented.as_bytes()) {
        return Decision::Rotate;
    }
    let is_previous = row
        .prev_refresh_hash
        .as_deref()
        .is_some_and(|previous| ct_eq(previous, presented.as_bytes()));
    if !is_previous {
        return Decision::Reject;
    }
    // A previous hash without a rotation time cannot be written; it is treated as a reuse.
    let within_grace = row.rotated_at_micros.is_some_and(|rotated| {
        now.checked_duration_since(Timestamp::from_unix_micros(rotated))
            .is_none_or(|elapsed| elapsed <= grace)
    });
    if within_grace {
        Decision::WithinGrace
    } else {
        Decision::Reuse
    }
}

fn reason_text(reason: RevokeReason) -> &'static str {
    match reason {
        RevokeReason::User => "user",
        RevokeReason::Admin => "admin",
        RevokeReason::RefreshReuse => "refresh_reuse",
        RevokeReason::AccountDeleted => "account_deleted",
    }
}

/// The result of the transaction of a refresh, with what the grace path needs to answer.
enum Outcome {
    Rotated,
    /// The previous token inside the grace window: the sealed successor and when its access token expires.
    Grace {
        sealed: Option<Vec<u8>>,
        access_expires_at_micros: i64,
    },
    Reuse,
    Reject,
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> SessionError {
    SessionError::Unexpected(Box::new(error))
}

fn after(at: Timestamp, by: Duration) -> Result<Timestamp, SessionError> {
    at.checked_add(by)
        .ok_or_else(|| unexpected(IdentityError::TimeOutOfRange))
}

#[async_trait]
impl SessionService for IdentitySessions {
    /// The previous token inside the grace window (the response to a rotation was lost) is answered with the
    /// same successor pair, opened from `successor_enc` with the presented token; nothing is created and
    /// nothing is revoked. A row without a sealed successor (rotated before the column existed) is refused.
    async fn refresh(&self, refresh_token: &RawToken) -> Result<SessionTokens, SessionError> {
        let presented = sha256_token(refresh_token.expose());
        let owner = auth_repo::session_by_refresh_hash(&self.db, &presented)
            .await
            .map_err(unexpected)?
            .ok_or(SessionError::Unauthenticated)?;
        let access = new_token(TokenKind::Session).map_err(unexpected)?;
        let refresh = new_token(TokenKind::Refresh).map_err(unexpected)?;
        let sealed = successor::seal(
            refresh_token.expose(),
            owner.session_id,
            &Pair {
                access: access.expose().to_owned(),
                refresh: refresh.expose().to_owned(),
            },
        )
        .map_err(unexpected)?;
        let now = self.clock.now_utc();
        let rotation = Rotation {
            token_hash: sha256_token(access.expose()),
            access_expires_at: after(now, self.access_token_ttl)?,
            refresh_hash: sha256_token(refresh.expose()),
            refresh_expires_at: after(now, self.refresh_token_ttl)?,
            rotated_at: now,
            successor_enc: sealed,
        };
        let grace = self.refresh_reuse_grace;
        let outcome = self
            .db
            .run(Scope::Account(owner.account_id), async |tx| {
                let Some(row) = session_repo::lock(tx, owner.session_id).await? else {
                    return Ok::<_, DbError>(Outcome::Reject);
                };
                Ok(match decide(&row, &presented, now, grace) {
                    Decision::Rotate => {
                        session_repo::rotate(tx, owner.session_id, &rotation).await?;
                        Outcome::Rotated
                    }
                    Decision::Reuse => {
                        session_repo::revoke(
                            tx,
                            owner.account_id,
                            owner.session_id,
                            now,
                            reason_text(RevokeReason::RefreshReuse),
                        )
                        .await?;
                        Outcome::Reuse
                    }
                    Decision::WithinGrace => Outcome::Grace {
                        sealed: row.successor_enc,
                        access_expires_at_micros: row.access_expires_at_micros,
                    },
                    Decision::Reject => Outcome::Reject,
                })
            })
            .await
            .map_err(unexpected)?;
        match outcome {
            Outcome::Rotated => Ok(SessionTokens {
                access,
                refresh,
                expires_in: self.access_token_ttl,
            }),
            Outcome::Grace {
                sealed,
                access_expires_at_micros,
            } => {
                let pair = sealed
                    .and_then(|sealed| {
                        successor::open(refresh_token.expose(), owner.session_id, &sealed)
                    })
                    .ok_or(SessionError::Unauthenticated)?;
                let remaining =
                    u64::try_from(access_expires_at_micros - now.as_unix_micros()).unwrap_or(0);
                Ok(SessionTokens {
                    access: RawToken::from_presented(TokenKind::Session, pair.access),
                    refresh: RawToken::from_presented(TokenKind::Refresh, pair.refresh),
                    expires_in: Duration::from_micros(remaining),
                })
            }
            Outcome::Reuse => {
                // Best effort on purpose, after the commit: a failed audit write must never undo the
                // security revocation of the reused session.
                tracing::warn!(
                    account_id = %owner.account_id,
                    session_id = %owner.session_id,
                    "refresh token reuse: session revoked"
                );
                record_logged(
                    &self.audit,
                    audit_events::refresh_reuse(owner.account_id, owner.session_id),
                )
                .await;
                Err(SessionError::ReuseDetected)
            }
            Outcome::Reject => Err(SessionError::Unauthenticated),
        }
    }

    async fn revoke(
        &self,
        account: Uuid,
        session_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), SessionError> {
        let (now, reason) = (self.clock.now_utc(), reason_text(reason));
        let audit = &self.audit;
        self.db
            .run(Scope::Account(account), async |tx| {
                if session_repo::revoke(tx, account, session_id, now, reason).await? > 0 {
                    let event = audit_events::session_revoked(account, Some(session_id), reason);
                    audit.record_in(tx, event).await?;
                }
                Ok::<_, TxError>(())
            })
            .await
            .map_err(unexpected)
    }

    async fn revoke_all(&self, account: Uuid, reason: RevokeReason) -> Result<(), SessionError> {
        let (now, reason) = (self.clock.now_utc(), reason_text(reason));
        let audit = &self.audit;
        self.db
            .run(Scope::Account(account), async |tx| {
                if session_repo::revoke_all(tx, account, now, reason).await? > 0 {
                    let event = audit_events::session_revoked(account, None, reason);
                    audit.record_in(tx, event).await?;
                }
                Ok::<_, TxError>(())
            })
            .await
            .map_err(unexpected)
    }

    async fn list(&self, account: Uuid) -> Result<Vec<SessionInfo>, SessionError> {
        let now = self.clock.now_utc().as_unix_micros();
        let rows = self
            .db
            .run(Scope::Account(account), async |tx| {
                session_repo::list(tx, account).await
            })
            .await
            .map_err(unexpected)?;
        Ok(rows
            .into_iter()
            .filter(|row| !row.revoked && row.refresh_expires_at_micros > now)
            .map(|row| SessionInfo {
                id: row.id,
                label: row.label,
                created_at: row.created_at,
                last_seen_at: row.last_seen_at,
            })
            .collect())
    }

    async fn touch(&self, principal: &Principal) -> Result<(), SessionError> {
        let now = self.clock.now_utc();
        let not_after = now
            .checked_sub(self.last_seen_interval)
            .ok_or_else(|| unexpected(IdentityError::TimeOutOfRange))?;
        self.db
            .run(Scope::Account(principal.account_id), async |tx| {
                session_repo::touch(
                    tx,
                    principal.kind,
                    principal.session_or_device_id,
                    now,
                    not_after,
                )
                .await
            })
            .await
            .map_err(unexpected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_000_000_000);
    const GRACE: Duration = Duration::from_millis(300);

    fn hash(label: &str) -> TokenHash {
        sha256_token(label)
    }

    /// A live session rotated `rotated_ago` before `NOW`: current token `current`, previous token `previous`.
    fn rotated(rotated_ago: Duration) -> LockedSession {
        LockedSession {
            refresh_hash: hash("current").as_bytes().to_vec(),
            prev_refresh_hash: Some(hash("previous").as_bytes().to_vec()),
            rotated_at_micros: Some(
                NOW.checked_sub(rotated_ago)
                    .expect("in range")
                    .as_unix_micros(),
            ),
            refresh_expires_at_micros: NOW.as_unix_micros() + 1_000_000,
            revoked: false,
            account_state: ACTIVE.to_owned(),
            successor_enc: None,
            access_expires_at_micros: NOW.as_unix_micros() + 900_000_000,
        }
    }

    #[test]
    fn current_token_rotates() {
        // Arrange
        let row = rotated(Duration::from_secs(5));

        // Act
        let decision = decide(&row, &hash("current"), NOW, GRACE);

        // Assert
        assert_eq!(decision, Decision::Rotate);
    }

    #[test]
    fn previous_token_inside_the_grace_changes_nothing() {
        // Arrange
        let row = rotated(GRACE);

        // Act
        let decision = decide(&row, &hash("previous"), NOW, GRACE);

        // Assert
        assert_eq!(decision, Decision::WithinGrace);
    }

    #[test]
    fn previous_token_after_the_grace_is_a_reuse() {
        // Arrange
        let row = rotated(GRACE + Duration::from_micros(1));

        // Act
        let decision = decide(&row, &hash("previous"), NOW, GRACE);

        // Assert
        assert_eq!(decision, Decision::Reuse);
    }

    #[test]
    fn older_generation_is_rejected() {
        // Arrange
        let row = rotated(Duration::from_secs(5));

        // Act
        let decision = decide(&row, &hash("older"), NOW, GRACE);

        // Assert
        assert_eq!(decision, Decision::Reject);
    }

    #[test]
    fn revoked_expired_and_inactive_sessions_reject_both_generations() {
        // Arrange
        let mut revoked = rotated(Duration::ZERO);
        revoked.revoked = true;
        let mut expired = rotated(Duration::ZERO);
        expired.refresh_expires_at_micros = NOW.as_unix_micros();
        let mut disabled = rotated(Duration::ZERO);
        disabled.account_state = "disabled".to_owned();

        // Act
        let decisions: Vec<Decision> = [revoked, expired, disabled]
            .iter()
            .flat_map(|row| {
                [
                    decide(row, &hash("current"), NOW, GRACE),
                    decide(row, &hash("previous"), NOW, GRACE),
                ]
            })
            .collect();

        // Assert
        assert_eq!(decisions, vec![Decision::Reject; 6]);
    }
}
