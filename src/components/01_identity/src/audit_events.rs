//! The identity audit events (01 §5, §7, §10) and how each is written.
//!
//! Mandatory events (owner decision 2026-11-13: account created, provider linked, sessions revoked, admin reset,
//! and the identity re-bound by the reset's invite) are written
//! with `AuditWriter::record_in` inside the transaction of the change, so a failed write rolls the change back
//! and the request fails. The other events (sign-in, device approval, refresh reuse, limiter trips) go through
//! [`record_logged`]: a failed write is logged at ERROR and the request outcome is unchanged. Every builder takes
//! ids and enumeration codes only; `AuditDetail` has no text value.

use std::net::IpAddr;

use contracts::Principal;
use sc_audit::{AuditDetail, AuditError, AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_db::DbError;
use sc_ratelimit::LimitId;
use thiserror::Error;
use uuid::Uuid;

const ACCOUNT: &str = "account";
const IDENTITY: &str = "identity";
const SESSION: &str = "session";
const DEVICE_GRANT: &str = "device_grant";

/// The failure of a transaction that writes a mandatory event: its statements or the audit insert.
#[derive(Debug, Error)]
pub(crate) enum TxError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Audit(#[from] AuditError),
}

/// Writes a non-mandatory event in its own short transaction; a failure is logged at ERROR with the event kind
/// and the request goes on.
pub(crate) async fn record_logged(audit: &AuditWriter, event: AuditEvent) {
    let kind = event.event;
    if let Err(error) = audit.record(event).await {
        tracing::error!(event = kind.as_str(), err = %error, "audit event not recorded");
    }
}

/// An account created from an invite (mandatory, in the account-creation transaction). `role` is the code of
/// the role the invite granted.
pub(crate) fn account_created(
    account: Uuid,
    identity: Uuid,
    invite: Uuid,
    role: &'static str,
) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(account),
        subject: Some(subject(ACCOUNT, account)),
        detail: AuditDetail::new()
            .with_code("action", "account_created")
            .with_id("identity_id", identity)
            .with_id("invite_id", invite)
            .with_code("role", role),
        ..AuditEvent::new(AuditEventKind::SignIn)
    }
}

/// The operator reset admin `account` and issued re-bind invite `invite` (mandatory, in the reset transaction).
/// The operator has no account, so the event has no actor.
pub(crate) fn admin_reset(account: Uuid, invite: Uuid) -> AuditEvent {
    AuditEvent {
        subject: Some(subject(ACCOUNT, account)),
        detail: AuditDetail::new()
            .with_code("action", "admin_reset")
            .with_id("invite_id", invite),
        ..AuditEvent::new(AuditEventKind::AdminAction)
    }
}

/// A re-bind invite redeemed: `identity` replaced the identities of `account` (mandatory, in the redemption
/// transaction).
pub(crate) fn identity_rebound(account: Uuid, identity: Uuid, invite: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(account),
        subject: Some(subject(IDENTITY, identity)),
        detail: AuditDetail::new()
            .with_code("action", "identity_rebound")
            .with_id("invite_id", invite),
        ..AuditEvent::new(AuditEventKind::SignIn)
    }
}

/// A provider identity linked to the signed-in account (mandatory, in the link transaction).
pub(crate) fn provider_linked(principal: &Principal, identity: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(principal.account_id),
        actor_session_id: Some(principal.session_or_device_id),
        subject: Some(subject(IDENTITY, identity)),
        detail: AuditDetail::new().with_code("action", "provider_linked"),
        ..AuditEvent::new(AuditEventKind::SignIn)
    }
}

/// A completed sign-in: `session` for a web sign-in, `None` for a mobile one (its session is minted at the code
/// exchange).
pub(crate) fn signed_in(account: Uuid, identity: Uuid, session: Option<Uuid>) -> AuditEvent {
    let action = if session.is_some() {
        "session_started"
    } else {
        "login_code_issued"
    };
    AuditEvent {
        actor_account_id: Some(account),
        actor_session_id: session,
        subject: Some(subject(IDENTITY, identity)),
        detail: AuditDetail::new().with_code("action", action),
        ..AuditEvent::new(AuditEventKind::SignIn)
    }
}

pub(crate) fn device_approved(approver: &Principal, grant: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(approver.account_id),
        actor_session_id: Some(approver.session_or_device_id),
        subject: Some(subject(DEVICE_GRANT, grant)),
        ..AuditEvent::new(AuditEventKind::DeviceApproved)
    }
}

/// One session revoked (`Some(session)`) or every session of the account (`None`); `reason` is the stored
/// revoke reason code.
pub(crate) fn session_revoked(
    account: Uuid,
    session: Option<Uuid>,
    reason: &'static str,
) -> AuditEvent {
    let (subject_ref, scope) = match session {
        Some(id) => (subject(SESSION, id), "one"),
        None => (subject(ACCOUNT, account), "all"),
    };
    AuditEvent {
        actor_account_id: Some(account),
        subject: Some(subject_ref),
        detail: AuditDetail::new()
            .with_code("reason", reason)
            .with_code("scope", scope),
        ..AuditEvent::new(AuditEventKind::SessionRevoked)
    }
}

/// A previous refresh token presented after the grace: the session was revoked.
pub(crate) fn refresh_reuse(account: Uuid, session: Uuid) -> AuditEvent {
    AuditEvent {
        actor_account_id: Some(account),
        subject: Some(subject(SESSION, session)),
        ..AuditEvent::new(AuditEventKind::RefreshReuse)
    }
}

/// One abuse-limit trip; written once per exhaustion, never per rejected call.
pub(crate) fn abuse_trip(
    limit: LimitId,
    actor: Option<&Principal>,
    ip: Option<IpAddr>,
) -> AuditEvent {
    AuditEvent {
        actor_account_id: actor.map(|principal| principal.account_id),
        actor_session_id: actor.map(|principal| principal.session_or_device_id),
        ip,
        detail: AuditDetail::new().with_code("limit", limit.label()),
        ..AuditEvent::new(AuditEventKind::AbuseTrip)
    }
}

fn subject(subject_type: &'static str, id: Uuid) -> SubjectRef {
    SubjectRef { subject_type, id }
}

#[cfg(test)]
mod tests {
    use contracts::{PrincipalKind, Role};
    use sc_audit::DetailValue;

    use super::*;

    fn is_code(text: &str) -> bool {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }

    fn principal() -> Principal {
        Principal {
            account_id: Uuid::from_u128(1),
            kind: PrincipalKind::Session,
            session_or_device_id: Uuid::from_u128(2),
            role: Role::User,
            label: "Firefox on alice@example.test".to_owned(),
        }
    }

    #[test]
    fn every_builder_carries_only_ids_and_enumeration_codes() {
        // Arrange
        let (a, b, c) = (Uuid::from_u128(3), Uuid::from_u128(4), Uuid::from_u128(5));
        let approver = principal();

        // Act
        let events = [
            account_created(a, b, c, "admin"),
            admin_reset(a, c),
            identity_rebound(a, b, c),
            provider_linked(&approver, b),
            signed_in(a, b, Some(c)),
            signed_in(a, b, None),
            device_approved(&approver, c),
            session_revoked(a, Some(b), "user"),
            session_revoked(a, None, "refresh_reuse"),
            refresh_reuse(a, b),
            abuse_trip(LimitId::DeviceApprovalIp, Some(&approver), None),
        ];

        // Assert
        for event in &events {
            for (key, value) in event.detail.entries() {
                assert!(is_code(key), "{key}");
                match value {
                    DetailValue::Id(_) | DetailValue::Int(_) => {}
                    DetailValue::Code(code) => assert!(is_code(code), "{code}"),
                }
            }
            let rendered = format!("{event:?}");
            assert!(
                !rendered.contains('@') && !rendered.contains("Firefox"),
                "{rendered}"
            );
            if let Some(subject) = event.subject {
                assert!(is_code(subject.subject_type));
            }
        }
    }

    #[test]
    fn a_trip_names_its_limit_and_the_caller_by_id() {
        // Arrange
        let approver = principal();

        // Act
        let event = abuse_trip(LimitId::DeviceApprovalSession, Some(&approver), None);

        // Assert
        assert_eq!(event.event, AuditEventKind::AbuseTrip);
        assert_eq!(event.actor_session_id, Some(approver.session_or_device_id));
        assert_eq!(
            event.detail.entries(),
            &[("limit", DetailValue::Code("device_approval_session"))]
        );
    }
}
