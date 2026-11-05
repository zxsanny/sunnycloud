use std::time::Duration;

use sc_clock::Timestamp;
use sc_db::Db;

use crate::error::AuditError;
use crate::repo;

/// Retention: the only delete of `audit_log`, through the `SECURITY DEFINER` function `audit_log_purge`.
#[derive(Clone)]
pub struct AuditPurge {
    db: Db,
    retention: Duration,
}

impl AuditPurge {
    /// `retention` is `audit_retention` from the settings; zero keeps every row.
    pub fn new(db: Db, retention: Duration) -> Self {
        AuditPurge { db, retention }
    }

    /// Deletes the rows older than `cutoff` and returns their number. With `audit_retention = 0` nothing is
    /// deleted and no statement runs.
    pub async fn purge_older_than(&self, cutoff: Timestamp) -> Result<u64, AuditError> {
        if self.retention.is_zero() {
            return Ok(0);
        }
        let removed = repo::unscoped(&self.db, async |tx| repo::purge(tx, cutoff).await).await?;
        Ok(removed)
    }
}
