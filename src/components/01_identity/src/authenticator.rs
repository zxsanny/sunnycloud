use std::sync::Arc;

use contracts::{
    Authenticator, AuthenticatorError, Credential, Fault, Principal, PrincipalKind,
    PrincipalRevalidator, RevalidatorError, Role, Scope, async_trait,
};
use sc_clock::{Clock, Timestamp};
use sc_db::Db;
use sc_ids::sha256_token;

use crate::IdentityConfig;
use crate::auth_repo::{self, CredentialRow};
use crate::error::IdentityError;
use crate::scope;
use crate::session_cache::SessionCache;

const ACTIVE: &str = "active";

/// Turns a session or device credential into a [`Principal`] and answers the long-poll recheck.
///
/// A cache miss is one indexed read; every dead credential (unknown, malformed, empty, expired, revoked,
/// of a disabled or deleting account) gives the same `Unauthenticated` after the same read. A revocation is
/// seen after at most `session_cache_ttl`.
pub struct IdentityAuthenticator {
    db: Db,
    clock: Arc<dyn Clock>,
    cache: SessionCache,
}

impl IdentityAuthenticator {
    pub fn new(db: Db, clock: Arc<dyn Clock>, config: &IdentityConfig) -> Self {
        let cache = SessionCache::new(config.session_cache_ttl, clock.monotonic());
        IdentityAuthenticator { db, clock, cache }
    }
}

/// Expiry, revocation and account state, decided here and not in the query.
fn is_live(row: &CredentialRow, now: Timestamp) -> bool {
    !row.revoked
        && row.account_state == ACTIVE
        && row
            .expires_at_micros
            .is_none_or(|expires| expires > now.as_unix_micros())
}

fn principal_of(row: CredentialRow, kind: PrincipalKind) -> Result<Principal, IdentityError> {
    let role = match row.role.as_str() {
        "user" => Role::User,
        "admin" => Role::Admin,
        _ => {
            return Err(IdentityError::UnexpectedColumnValue {
                column: "accounts.role",
                value: row.role,
            });
        }
    };
    Ok(Principal {
        account_id: row.account_id,
        kind,
        session_or_device_id: row.id,
        role,
        label: row.label,
    })
}

fn fault(error: impl std::error::Error + Send + Sync + 'static) -> Fault {
    Box::new(error)
}

#[async_trait]
impl Authenticator for IdentityAuthenticator {
    async fn authenticate(&self, credential: Credential) -> Result<Principal, AuthenticatorError> {
        let (kind, token) = match &credential {
            Credential::Session(token) => (PrincipalKind::Session, token),
            Credential::Device(token) => (PrincipalKind::Device, token),
        };
        let hash = sha256_token(token);
        let read_at = self.clock.monotonic();
        if let Some(principal) = self.cache.principal(&hash, kind, read_at) {
            return Ok(principal);
        }
        let row = auth_repo::by_hash(&self.db, kind, &hash)
            .await
            .map_err(|error| AuthenticatorError::Unexpected(fault(error)))?
            .filter(|row| is_live(row, self.clock.now_utc()))
            .ok_or(AuthenticatorError::Unauthenticated)?;
        let principal = principal_of(row, kind)
            .map_err(|error| AuthenticatorError::Unexpected(fault(error)))?;
        self.cache.store_principal(hash, &principal, read_at);
        Ok(principal)
    }

    fn require(&self, principal: &Principal, scope: Scope) -> Result<(), AuthenticatorError> {
        scope::require(principal, scope)
    }
}

#[async_trait]
impl PrincipalRevalidator for IdentityAuthenticator {
    async fn revalidate(&self, principal: &Principal) -> Result<bool, RevalidatorError> {
        let read_at = self.clock.monotonic();
        if let Some(alive) = self.cache.alive(principal, read_at) {
            return Ok(alive);
        }
        let alive = auth_repo::by_id(
            &self.db,
            principal.kind,
            principal.account_id,
            principal.session_or_device_id,
        )
        .await
        .map_err(|error| RevalidatorError::Unexpected(fault(error)))?
        .is_some_and(|row| is_live(&row, self.clock.now_utc()));
        self.cache.store_liveness(principal, alive, read_at);
        Ok(alive)
    }
}
