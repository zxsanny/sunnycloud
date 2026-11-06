use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    ApprovalView, Decision, DeviceAuthorization, DeviceGrantError, DeviceGrantService, DeviceInfo,
    DeviceToken, Principal, PrincipalKind, Scope, async_trait,
};
use sc_clock::{Clock, Timestamp};
use sc_db::{DbError, RunError, Scope as DbScope};
use sc_ids::{RawToken, TokenKind, UserCode, new_id, new_token, new_user_code, sha256_token};
use uuid::Uuid;

use crate::auth_repo;
use crate::config::IdentityConfig;
use crate::device_grant_repo::{
    self, Approver, DecideOutcome, GrantState, NewDeviceToken, NewGrant, PollStep,
};
use crate::error::IdentityError;
use crate::geo::GeoLookup;
use crate::scope;

const USER_CODE_KEY: &str = "device_grants_user_code_key";
/// A collision of a fresh 8-letter code with a pending one is rare; this many draws all colliding is a fault.
const USER_CODE_DRAWS: usize = 5;

/// The RFC 8628 device grant of the desktop client, and the device list.
///
/// A poll is one short system-scope transaction: the conditional update that moves the grant from `approved`
/// to `consumed` and the insert of the device token commit together, so a code exchanges once and never after
/// its expiry.
pub struct IdentityDeviceGrants {
    db: sc_db::Db,
    clock: Arc<dyn Clock>,
    geo: Arc<dyn GeoLookup>,
    code_ttl: Duration,
    poll_interval: Duration,
}

impl IdentityDeviceGrants {
    pub fn new(
        db: sc_db::Db,
        clock: Arc<dyn Clock>,
        geo: Arc<dyn GeoLookup>,
        config: &IdentityConfig,
    ) -> Self {
        IdentityDeviceGrants {
            db,
            clock,
            geo,
            code_ttl: config.device_code_ttl,
            poll_interval: config.device_poll_interval,
        }
    }
}

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> DeviceGrantError {
    DeviceGrantError::Unexpected(Box::new(error))
}

fn after(at: Timestamp, by: Duration) -> Result<Timestamp, DeviceGrantError> {
    at.checked_add(by)
        .ok_or_else(|| unexpected(IdentityError::TimeOutOfRange))
}

fn require_session(principal: &Principal) -> Result<(), DeviceGrantError> {
    scope::require(principal, Scope::SessionOnly).map_err(|_| DeviceGrantError::Forbidden)
}

/// The approver is warned when the request came from another address.
fn is_mismatch(requester: IpAddr, approver: IpAddr) -> bool {
    requester != approver
}

/// Why a poll that minted nothing and touched nothing is refused. Expiry comes before the status: a code past
/// its expiry is expired whatever was decided, and an approved grant is not minted after it.
fn refusal(state: &GrantState, now: Timestamp) -> DeviceGrantError {
    if state.expires_at_micros <= now.as_unix_micros() {
        return DeviceGrantError::ExpiredToken;
    }
    match state.status.as_str() {
        "pending" => DeviceGrantError::SlowDown,
        "approved" => DeviceGrantError::AuthorizationPending,
        "denied" => DeviceGrantError::AccessDenied,
        "consumed" => DeviceGrantError::ExpiredToken,
        other => unexpected(IdentityError::UnexpectedColumnValue {
            column: "device_grants.status",
            value: other.to_owned(),
        }),
    }
}

#[async_trait]
impl DeviceGrantService for IdentityDeviceGrants {
    async fn request(
        &self,
        device_name: &str,
        platform: &str,
        requester_ip: IpAddr,
    ) -> Result<DeviceAuthorization, DeviceGrantError> {
        let ip = requester_ip.to_canonical();
        let device_code = new_token(TokenKind::DeviceCode).map_err(unexpected)?;
        let device_code_hash = sha256_token(device_code.expose());
        let location = self.geo.locate(ip);
        let created_at = self.clock.now_utc();
        let expires_at = after(created_at, self.code_ttl)?;
        for _ in 0..USER_CODE_DRAWS {
            let user_code = new_user_code().map_err(unexpected)?;
            let grant = NewGrant {
                id: new_id(self.clock.as_ref()),
                device_code_hash: &device_code_hash,
                user_code: user_code.as_str(),
                requester_ip: ip.to_string(),
                requester_location: location.as_deref(),
                device_name,
                platform,
                created_at,
                expires_at,
            };
            match auth_repo::insert_grant(&self.db, &grant).await {
                Ok(()) => {
                    return Ok(DeviceAuthorization {
                        device_code,
                        user_code,
                        interval: self.poll_interval,
                        expires_in: self.code_ttl,
                    });
                }
                Err(RunError::Closure(DbError::UniqueViolation { constraint }))
                    if constraint.as_deref() == Some(USER_CODE_KEY) => {}
                Err(error) => return Err(unexpected(error)),
            }
        }
        Err(unexpected(IdentityError::UserCodeExhausted))
    }

    async fn poll(&self, device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError> {
        let code_hash = sha256_token(device_code.expose());
        let token = new_token(TokenKind::DeviceToken).map_err(unexpected)?;
        let token_hash = sha256_token(token.expose());
        let now = self.clock.now_utc();
        let polled_before = now
            .checked_sub(self.poll_interval)
            .ok_or_else(|| unexpected(IdentityError::TimeOutOfRange))?;
        let minted = NewDeviceToken {
            id: new_id(self.clock.as_ref()),
            token_hash: &token_hash,
            created_at: now,
        };
        let step = auth_repo::poll_grant(&self.db, &code_hash, &minted, now, polled_before)
            .await
            .map_err(unexpected)?;
        match step {
            PollStep::Minted => Ok(DeviceToken(token)),
            PollStep::Touched => Err(DeviceGrantError::AuthorizationPending),
            PollStep::Refused(None) => Err(DeviceGrantError::ExpiredToken),
            PollStep::Refused(Some(state)) => Err(refusal(&state, now)),
        }
    }

    async fn lookup_for_approval(
        &self,
        principal: &Principal,
        user_code: &UserCode,
        approver_ip: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        require_session(principal)?;
        let approver_ip = approver_ip.to_canonical();
        let now = self.clock.now_utc();
        let grant = auth_repo::pending_grant_by_user_code(&self.db, user_code.as_str(), now)
            .await
            .map_err(unexpected)?
            .ok_or(DeviceGrantError::UnknownCode)?;
        let requester_ip: IpAddr = grant.requester_ip.parse().map_err(|_| {
            unexpected(IdentityError::UnexpectedColumnValue {
                column: "device_grants.requester_ip",
                value: grant.requester_ip.clone(),
            })
        })?;
        Ok(ApprovalView {
            grant_id: grant.id,
            expires_at: Timestamp::from_unix_micros(grant.expires_at_micros),
            device: grant.device_name,
            requester_ip,
            requester_location: grant.requester_location,
            approver_ip,
            approver_location: self.geo.locate(approver_ip),
            mismatch: is_mismatch(requester_ip, approver_ip),
        })
    }

    async fn decide(
        &self,
        principal: &Principal,
        grant_id: Uuid,
        decision: Decision,
    ) -> Result<(), DeviceGrantError> {
        require_session(principal)?;
        let approver = Approver {
            account_id: principal.account_id,
            session_id: principal.session_or_device_id,
        };
        let outcome = auth_repo::decide_grant(
            &self.db,
            grant_id,
            decision,
            &approver,
            self.clock.now_utc(),
        )
        .await
        .map_err(unexpected)?;
        match outcome {
            DecideOutcome::Applied => Ok(()),
            DecideOutcome::Expired => Err(DeviceGrantError::Expired),
            DecideOutcome::AlreadyDecided => Err(DeviceGrantError::AlreadyDecided),
            DecideOutcome::Unknown => Err(DeviceGrantError::UnknownCode),
        }
    }

    async fn list_devices(&self, account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError> {
        let rows = self
            .db
            .run(DbScope::Account(account), async |tx| {
                device_grant_repo::list_devices(tx, account).await
            })
            .await
            .map_err(unexpected)?;
        Ok(rows
            .into_iter()
            .map(|row| DeviceInfo {
                id: row.id,
                name: row.name,
                platform: row.platform,
                created_at: row.created_at,
                last_seen_at: row.last_seen_at,
            })
            .collect())
    }

    async fn revoke_device(&self, account: Uuid, device_id: Uuid) -> Result<(), DeviceGrantError> {
        let revoked = self.revoke(account, device_id).await?;
        if revoked {
            Ok(())
        } else {
            Err(DeviceGrantError::NotFound)
        }
    }

    /// A session principal is refused, and nothing is revoked. An already revoked device is a no-op.
    async fn revoke_self(&self, principal: &Principal) -> Result<(), DeviceGrantError> {
        if principal.kind != PrincipalKind::Device {
            return Err(DeviceGrantError::Forbidden);
        }
        self.revoke(principal.account_id, principal.session_or_device_id)
            .await
            .map(drop)
    }
}

impl IdentityDeviceGrants {
    async fn revoke(&self, account: Uuid, device_id: Uuid) -> Result<bool, DeviceGrantError> {
        let at = self.clock.now_utc();
        self.db
            .run(DbScope::Account(account), async |tx| {
                device_grant_repo::revoke_device(tx, account, device_id, at).await
            })
            .await
            .map_err(unexpected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_000_000_000);

    fn state(status: &str, expires_in_micros: i64) -> GrantState {
        GrantState {
            status: status.to_owned(),
            expires_at_micros: NOW.as_unix_micros() + expires_in_micros,
        }
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("test address")
    }

    #[test]
    fn different_addresses_are_a_mismatch_and_the_same_address_is_not() {
        // Arrange
        let requester = ip("203.0.113.7");

        // Act
        let other = is_mismatch(requester, ip("198.51.100.9"));
        let same = is_mismatch(requester, ip("203.0.113.7"));

        // Assert
        assert!(other);
        assert!(!same);
    }

    #[test]
    fn a_live_grant_is_refused_by_its_status() {
        // Arrange
        let cases = [
            ("pending", "SlowDown"),
            ("approved", "AuthorizationPending"),
            ("denied", "AccessDenied"),
            ("consumed", "ExpiredToken"),
        ];

        // Act
        let refusals: Vec<String> = cases
            .iter()
            .map(|(status, _)| format!("{:?}", refusal(&state(status, 1_000), NOW)))
            .collect();

        // Assert
        let expected: Vec<String> = cases.iter().map(|(_, name)| (*name).to_owned()).collect();
        assert_eq!(refusals, expected);
    }

    #[test]
    fn a_grant_at_or_past_its_expiry_is_expired_whatever_its_status() {
        // Arrange
        let statuses = ["pending", "approved", "denied", "consumed"];

        // Act
        let refusals: Vec<String> = statuses
            .iter()
            .flat_map(|status| [state(status, 0), state(status, -1)])
            .map(|state| format!("{:?}", refusal(&state, NOW)))
            .collect();

        // Assert
        assert_eq!(refusals, vec!["ExpiredToken"; 8]);
    }
}
