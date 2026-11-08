//! What the reader does when it meets a corrupt or missing blob: mark the row, then one audit event.

use std::sync::Arc;

use contracts::BlobError;
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_clock::Clock;
use sc_db::Db;
use sc_observability::log_error;
use uuid::Uuid;

use super::finding_repo;
use crate::error::{BlobIds, blob_error_from_db, event_fault};

pub(super) struct Findings {
    db: Db,
    audit: AuditWriter,
    clock: Arc<dyn Clock>,
}

impl Findings {
    pub fn new(db: Db, audit: AuditWriter, clock: Arc<dyn Clock>) -> Self {
        Findings { db, audit, clock }
    }

    /// `Corrupt` and `BlobMissing` are findings; any other outcome is not. The mark is the guard: it changes the
    /// row only for the first finding of a committed blob, so a second finding, in this process or another,
    /// writes no second event. A failed mark is logged and the read still fails with its own error.
    pub async fn note(&self, blob_id: Uuid, error: &BlobError) {
        let reason = match error {
            BlobError::Corrupt => {
                let ids = BlobIds {
                    blob_id: Some(blob_id),
                    account_id: None,
                };
                log_error!(
                    "blob corrupt on read",
                    err = &*event_fault("corrupt chunk", ids)
                );
                "corrupt"
            }
            BlobError::BlobMissing => "missing",
            _ => return,
        };
        let at = self.clock.now_utc().as_unix_micros();
        match finding_repo::mark_unavailable(&self.db, blob_id, at).await {
            Ok(Some(account)) => {
                let event = AuditEvent {
                    subject: Some(SubjectRef {
                        subject_type: "blob",
                        id: blob_id,
                    }),
                    detail: AuditDetail::new()
                        .with_id("account", account)
                        .with_code("reason", reason)
                        .with_code("source", "reader"),
                    ..AuditEvent::new(AuditEventKind::ScrubMismatch)
                };
                self.audit.record_best_effort(event).await;
            }
            Ok(None) => {}
            Err(error) => {
                let cause = blob_error_from_db(
                    error,
                    BlobIds {
                        blob_id: Some(blob_id),
                        account_id: None,
                    },
                );
                log_error!("blob finding not recorded", err = &cause);
            }
        }
    }
}
