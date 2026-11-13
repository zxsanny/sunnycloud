//! The AC-20 abuse counters of identity on the shared `AbuseLimiter` (built once by 12): wrong user codes at
//! device approval (per approving session, per account, per approver address), failed sign-in steps per
//! address, and invalid tokens presented at sign-in per address. Callers check before the work and record after
//! a failure; while any dimension is limited the caller is refused, even with a correct code or token.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::Principal;
use sc_audit::AuditWriter;
use sc_ratelimit::{AbuseLimiter, LimitId, Subject, Verdict};

use crate::audit_events::{self, record_logged};

pub(crate) struct AbuseCounter {
    limiter: Arc<AbuseLimiter>,
    audit: AuditWriter,
}

/// The three device-approval dimensions of one approver.
pub(crate) fn approval_dimensions(approver: &Principal, ip: IpAddr) -> [(LimitId, Subject); 3] {
    [
        (
            LimitId::DeviceApprovalSession,
            Subject::Session(approver.session_or_device_id),
        ),
        (
            LimitId::DeviceApprovalAccount,
            Subject::Account(approver.account_id),
        ),
        (LimitId::DeviceApprovalIp, Subject::Ip(ip)),
    ]
}

impl AbuseCounter {
    pub(crate) fn new(limiter: Arc<AbuseLimiter>, audit: AuditWriter) -> Self {
        AbuseCounter { limiter, audit }
    }

    /// Reads only. The longest wait of the limited dimensions, `None` when the caller may go on.
    pub(crate) fn retry_after(&self, dimensions: &[(LimitId, Subject)]) -> Option<Duration> {
        dimensions
            .iter()
            .filter_map(
                |(limit, subject)| match self.limiter.check(*limit, subject) {
                    Verdict::Allowed => None,
                    Verdict::Tripped { retry_after } | Verdict::Limited { retry_after } => {
                        Some(retry_after)
                    }
                },
            )
            .max()
    }

    /// Counts one failure on every dimension. A dimension this call exhausts writes one `abuse_trip` event and
    /// one WARN line; the WARN line carries the limit and the caller's ids only (no address).
    pub(crate) async fn record(
        &self,
        dimensions: &[(LimitId, Subject)],
        actor: Option<&Principal>,
        ip: IpAddr,
    ) {
        for (limit, subject) in dimensions {
            if let Verdict::Tripped { .. } = self.limiter.record_failure(*limit, subject) {
                match actor {
                    Some(principal) => tracing::warn!(
                        limit = limit.label(),
                        account_id = %principal.account_id,
                        session_id = %principal.session_or_device_id,
                        "abuse limit tripped"
                    ),
                    None => tracing::warn!(limit = limit.label(), "abuse limit tripped"),
                }
                record_logged(
                    &self.audit,
                    audit_events::abuse_trip(*limit, actor, Some(ip)),
                )
                .await;
            }
        }
    }
}
