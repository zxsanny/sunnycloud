use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sc_clock::Clock;
use sc_db::{Db, DbTx};

use crate::error::AuditError;
use crate::event::AuditEvent;
use crate::repo;

/// Snapshot of [`AuditWriter::stats`], the source of `sc_audit_write_failures_total`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditStats {
    /// Best-effort writes that did not reach the table (store failure or rejected shape) since start.
    pub write_failures: u64,
}

/// The one writer of `audit_log`. Cheap to clone; clones share the failure counter.
///
/// State changes use [`AuditWriter::record_in`] as the last statement of their transaction, or
/// [`AuditWriter::record`]. Only `share_accessed`, `scrub_mismatch` and `scrub_summary` use
/// [`AuditWriter::record_best_effort`].
#[derive(Clone)]
pub struct AuditWriter {
    db: Db,
    clock: Arc<dyn Clock>,
    failures: Arc<AtomicU64>,
}

impl AuditWriter {
    /// `db` is the API-role handle; `clock` stamps `at` (SQL never reads the clock).
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        AuditWriter {
            db,
            clock,
            failures: Arc::default(),
        }
    }

    /// Inserts the event inside the caller's transaction: one statement. A failure is returned, and the caller
    /// returns it from its unit of work, so the business change rolls back with it. Call it last, right before
    /// the commit.
    pub async fn record_in(&self, tx: &mut DbTx, event: AuditEvent) -> Result<(), AuditError> {
        let detail = self.insert_in(tx, &event).await?;
        mirror(&event, detail.as_deref());
        Ok(())
    }

    /// Inserts the event in its own short transaction.
    pub async fn record(&self, event: AuditEvent) -> Result<(), AuditError> {
        let detail = self.insert_own(&event).await?;
        mirror(&event, detail.as_deref());
        Ok(())
    }

    /// Never fails. A failure is logged at ERROR with the event kind and counted in [`AuditWriter::stats`].
    pub async fn record_best_effort(&self, event: AuditEvent) {
        let kind = event.event;
        match self.insert_own(&event).await {
            Ok(detail) => mirror(&event, detail.as_deref()),
            Err(error) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                tracing::error!(event = kind.as_str(), err = %error, "audit event not recorded (best effort)");
            }
        }
    }

    pub fn stats(&self) -> AuditStats {
        AuditStats {
            write_failures: self.failures.load(Ordering::Relaxed),
        }
    }

    // Both return the rendered detail, which the mirror line carries.
    async fn insert_in(
        &self,
        tx: &mut DbTx,
        event: &AuditEvent,
    ) -> Result<Option<String>, AuditError> {
        let detail = event.validate()?;
        repo::insert(tx, self.clock.now_utc(), event, detail.as_deref()).await?;
        Ok(detail)
    }

    async fn insert_own(&self, event: &AuditEvent) -> Result<Option<String>, AuditError> {
        let detail = event.validate()?;
        let at = self.clock.now_utc();
        repo::unscoped(&self.db, async |tx| {
            repo::insert(tx, at, event, detail.as_deref()).await
        })
        .await?;
        Ok(detail)
    }
}

/// One log line per recorded event: target `audit`, which the logging layer renders with `"stream":"audit"`
/// (`sc_observability` contract). Only ids, codes, integers and the client address are fields.
fn mirror(event: &AuditEvent, detail: Option<&str>) {
    tracing::info!(
        target: "audit",
        event = event.event.as_str(),
        actor_account_id = event.actor_account_id.map(|id| id.to_string()).as_deref(),
        actor_session_id = event.actor_session_id.map(|id| id.to_string()).as_deref(),
        subject_type = event.subject.map(|subject| subject.subject_type),
        subject_id = event.subject.map(|subject| subject.id.to_string()).as_deref(),
        ip = event.ip.map(|ip| ip.to_string()).as_deref(),
        detail,
        "audit"
    );
}
