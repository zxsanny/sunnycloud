use std::time::Duration;

use contracts::{CycleStats, Janitor, JanitorError, async_trait};
use sc_audit::AuditPurge;
use sc_clock::Timestamp;
use sc_db::Db;

/// Deletes the audit rows older than `audit_retention`. Helpers carry no `contracts` port, so this wrapper
/// lives in the composition root (module-layout open point 6).
pub struct AuditRetentionJanitor {
    purge: AuditPurge,
    retention: Duration,
}

impl AuditRetentionJanitor {
    /// `retention` is the `audit_retention` setting; zero keeps every row.
    pub fn new(db: Db, retention: Duration) -> Self {
        AuditRetentionJanitor {
            purge: AuditPurge::new(db, retention),
            retention,
        }
    }
}

#[async_trait]
impl Janitor for AuditRetentionJanitor {
    fn name(&self) -> &'static str {
        "audit_retention"
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        // A retention longer than the clock's range leaves no row old enough.
        let Some(cutoff) = now.checked_sub(self.retention) else {
            return Ok(CycleStats::default());
        };
        let removed = self
            .purge
            .purge_older_than(cutoff)
            .await
            .map_err(|error| JanitorError::Unexpected(Box::new(error)))?;
        Ok(CycleStats { processed: removed })
    }
}
