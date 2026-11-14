//! The AC-20 abuse counters of sharing on the shared `AbuseLimiter` (built once by 12; 06 never builds one):
//!
//! - invalid tokens per client address (`ShareTokenIp`, `share_invalid_token_limit` per minute): checked before
//!   the lookup of `open`, `authorize_content`, `authorize_derived` and `verify_password`; every `NotFound` answer
//!   of the four counts (an unknown token is an invalid token whatever the route), so the not-found cases stay
//!   alike, and a limited address is refused on every token, a live one included, so the limit cannot confirm a
//!   guess;
//! - wrong passwords per link (`SharePasswordLink`, `share_wrong_password_limit` per
//!   `share_wrong_password_window`): a password attempt takes one token of the link's bucket before the PHC read and
//!   Argon2 (an atomic reservation, so parallel attempts from any number of addresses cannot all pass a read-only
//!   check); an empty bucket answers `RateLimited` even for the right password (G-06-2), and the right password
//!   clears the link's counter, so only wrong passwords stay counted.
//!
//! Each exhaustion writes one `abuse_trip` audit event (best effort) and one WARN line with the link id or a hash of
//! the address; the token and the password are never in either.

use std::net::IpAddr;

use contracts::PublicShareError;
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, SubjectRef};
use sc_ids::sha256_token;
use sc_ratelimit::{LimitId, Subject, Verdict};
use uuid::Uuid;

use crate::service::{PgShareService, SHARE_LINK};

/// A password attempt that holds one token of its link's bucket; `exhausts` when it took the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PasswordAttempt {
    link: Uuid,
    exhausts: bool,
}

impl PgShareService {
    /// Reads only: `RateLimited` while `ip` has used up its invalid tokens.
    pub(crate) fn check_token_ip(&self, ip: IpAddr) -> Result<(), PublicShareError> {
        match self.limiter.check(LimitId::ShareTokenIp, &Subject::Ip(ip)) {
            Verdict::Allowed => Ok(()),
            Verdict::Tripped { retry_after } | Verdict::Limited { retry_after } => {
                Err(PublicShareError::RateLimited { retry_after })
            }
        }
    }

    /// Counts one invalid token of `ip`; the call that exhausts the address audits the trip.
    pub(crate) async fn count_invalid_token(&self, ip: IpAddr) {
        let verdict = self
            .limiter
            .record_failure(LimitId::ShareTokenIp, &Subject::Ip(ip));
        if let Verdict::Tripped { .. } = verdict {
            tracing::warn!(
                limit = LimitId::ShareTokenIp.label(),
                ip_hash = %ip_hash(ip),
                "share abuse limit tripped"
            );
            self.audit
                .record_best_effort(trip_event(LimitId::ShareTokenIp, None, ip))
                .await;
        }
    }

    /// Takes one token of `link`'s bucket before any password work; `RateLimited` when it is empty.
    pub(crate) fn reserve_password_attempt(
        &self,
        link: Uuid,
    ) -> Result<PasswordAttempt, PublicShareError> {
        match self
            .limiter
            .record_failure(LimitId::SharePasswordLink, &Subject::Link(link))
        {
            Verdict::Allowed => Ok(PasswordAttempt {
                link,
                exhausts: false,
            }),
            Verdict::Tripped { .. } => Ok(PasswordAttempt {
                link,
                exhausts: true,
            }),
            Verdict::Limited { retry_after } => Err(PublicShareError::RateLimited { retry_after }),
        }
    }

    /// The attempt's password was wrong: its token stays taken; the attempt that exhausted the link audits the trip.
    pub(crate) async fn wrong_password(&self, attempt: PasswordAttempt, ip: IpAddr) {
        if attempt.exhausts {
            tracing::warn!(
                limit = LimitId::SharePasswordLink.label(),
                link_id = %attempt.link,
                "share abuse limit tripped"
            );
            self.audit
                .record_best_effort(trip_event(
                    LimitId::SharePasswordLink,
                    Some(attempt.link),
                    ip,
                ))
                .await;
        }
    }

    /// The attempt's password was right: the link's wrong passwords are forgotten.
    pub(crate) fn right_password(&self, attempt: PasswordAttempt) {
        self.limiter
            .clear(LimitId::SharePasswordLink, &Subject::Link(attempt.link));
    }
}

/// One trip: the limit, the address that exhausted it and, for the per-link limit, the link. No actor: the visitor
/// has no account.
pub(crate) fn trip_event(limit: LimitId, link: Option<Uuid>, ip: IpAddr) -> AuditEvent {
    AuditEvent {
        subject: link.map(|id| SubjectRef {
            subject_type: SHARE_LINK,
            id,
        }),
        ip: Some(ip),
        detail: AuditDetail::new().with_code("limit", limit.label()),
        ..AuditEvent::new(AuditEventKind::AbuseTrip)
    }
}

/// The first 8 bytes of the SHA-256 of the address text, in hex. A pseudonym for log correlation, not anonymity: the
/// IPv4 space can be enumerated.
pub(crate) fn ip_hash(ip: IpAddr) -> String {
    let digest = sha256_token(ip.to_string()).into_bytes();
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    format!("{:016x}", u64::from_be_bytes(head))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));

    #[test]
    fn a_trip_event_names_the_limit_the_address_and_the_link_only() {
        // Arrange
        let link = Uuid::from_u128(8);

        // Act
        let per_link = trip_event(LimitId::SharePasswordLink, Some(link), IP);
        let per_ip = trip_event(LimitId::ShareTokenIp, None, IP);

        // Assert
        assert_eq!(per_link.event, AuditEventKind::AbuseTrip);
        assert_eq!(
            per_link.subject,
            Some(SubjectRef {
                subject_type: SHARE_LINK,
                id: link
            })
        );
        assert_eq!(per_ip.subject, None);
        for (event, label) in [(&per_link, "share_password"), (&per_ip, "share_token_ip")] {
            assert_eq!(event.ip, Some(IP));
            assert_eq!(
                (event.actor_account_id, event.actor_session_id),
                (None, None)
            );
            let keys: Vec<&str> = event.detail.entries().iter().map(|(key, _)| *key).collect();
            assert_eq!(keys, ["limit"]);
            assert!(format!("{:?}", event.detail).contains(label));
        }
    }

    #[test]
    fn the_address_hash_is_stable_short_hex_and_never_the_address() {
        // Arrange
        let other = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8));

        // Act
        let (first, again, second) = (ip_hash(IP), ip_hash(IP), ip_hash(other));

        // Assert
        assert_eq!(first, again);
        assert_ne!(first, second);
        assert_eq!(first.len(), 16);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!first.contains("203"));
    }
}
