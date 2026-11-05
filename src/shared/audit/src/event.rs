use std::net::IpAddr;

use uuid::Uuid;

use crate::detail::{AuditDetail, is_token};
use crate::error::Rejection;

/// The 14 event kinds of `audit_log.event` (named check `audit_log_event_check`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditEventKind {
    SignIn,
    DeviceApproved,
    DeviceRevoked,
    SessionRevoked,
    RefreshReuse,
    ShareCreated,
    ShareRevoked,
    ShareAccessed,
    AdminAction,
    AccountDeleted,
    AbuseTrip,
    VaultFailure,
    ScrubMismatch,
    ScrubSummary,
}

impl AuditEventKind {
    pub const ALL: [AuditEventKind; 14] = [
        AuditEventKind::SignIn,
        AuditEventKind::DeviceApproved,
        AuditEventKind::DeviceRevoked,
        AuditEventKind::SessionRevoked,
        AuditEventKind::RefreshReuse,
        AuditEventKind::ShareCreated,
        AuditEventKind::ShareRevoked,
        AuditEventKind::ShareAccessed,
        AuditEventKind::AdminAction,
        AuditEventKind::AccountDeleted,
        AuditEventKind::AbuseTrip,
        AuditEventKind::VaultFailure,
        AuditEventKind::ScrubMismatch,
        AuditEventKind::ScrubSummary,
    ];

    /// The value stored in `audit_log.event`.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEventKind::SignIn => "sign_in",
            AuditEventKind::DeviceApproved => "device_approved",
            AuditEventKind::DeviceRevoked => "device_revoked",
            AuditEventKind::SessionRevoked => "session_revoked",
            AuditEventKind::RefreshReuse => "refresh_reuse",
            AuditEventKind::ShareCreated => "share_created",
            AuditEventKind::ShareRevoked => "share_revoked",
            AuditEventKind::ShareAccessed => "share_accessed",
            AuditEventKind::AdminAction => "admin_action",
            AuditEventKind::AccountDeleted => "account_deleted",
            AuditEventKind::AbuseTrip => "abuse_trip",
            AuditEventKind::VaultFailure => "vault_failure",
            AuditEventKind::ScrubMismatch => "scrub_mismatch",
            AuditEventKind::ScrubSummary => "scrub_summary",
        }
    }

    pub fn from_code(code: &str) -> Option<AuditEventKind> {
        AuditEventKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == code)
    }
}

/// What an event is about: a type (an identifier such as `share_link`; the writer epics own the list) and an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectRef {
    pub subject_type: &'static str,
    pub id: Uuid,
}

/// One security event. Every field is an id, an enumeration, an address or an integer; there is no text field.
///
/// ```compile_fail,E0560
/// let _event = sc_audit::AuditEvent {
///     name: String::from("alice"),
///     ..sc_audit::AuditEvent::new(sc_audit::AuditEventKind::SignIn)
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub event: AuditEventKind,
    pub actor_account_id: Option<Uuid>,
    pub actor_session_id: Option<Uuid>,
    pub subject: Option<SubjectRef>,
    pub ip: Option<IpAddr>,
    pub detail: AuditDetail,
}

impl AuditEvent {
    /// An event with no actor, subject, address or detail; set the rest with struct update syntax.
    pub fn new(event: AuditEventKind) -> Self {
        AuditEvent {
            event,
            actor_account_id: None,
            actor_session_id: None,
            subject: None,
            ip: None,
            detail: AuditDetail::new(),
        }
    }

    /// The shape check shared by every writer: returns the `jsonb` text of the detail.
    pub(crate) fn validate(&self) -> Result<Option<String>, Rejection> {
        if let Some(subject) = &self.subject
            && !is_token(subject.subject_type)
        {
            return Err(Rejection::InvalidToken {
                place: "subject type",
            });
        }
        self.detail.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_round_trip_and_are_the_fourteen_stored_values() {
        let codes: Vec<&str> = AuditEventKind::ALL
            .iter()
            .map(|kind| kind.as_str())
            .collect();

        assert_eq!(codes.len(), 14);
        for kind in AuditEventKind::ALL {
            assert_eq!(AuditEventKind::from_code(kind.as_str()), Some(kind));
        }
        assert_eq!(AuditEventKind::from_code("unknown"), None);
    }

    #[test]
    fn a_subject_type_that_is_not_a_token_is_rejected() {
        let event = AuditEvent {
            subject: Some(SubjectRef {
                subject_type: "Share Link",
                id: Uuid::nil(),
            }),
            ..AuditEvent::new(AuditEventKind::ShareCreated)
        };

        assert_eq!(
            event.validate(),
            Err(Rejection::InvalidToken {
                place: "subject type"
            })
        );
    }
}
