//! What the scrubber does with a finding: decide whether it is one, then mark the row and write the audit event.
//!
//! A file the scrubber could not read may belong to a collector at that moment. The GC sweeper schedules a blob
//! (`gc_pending`) before it parks the file as `<id>.gc`, and moves the file back for a blob a reinstate has made
//! `committed` again, so a missing file is first looked for under both names, then the row is re-read in a new
//! short transaction. Only a blob that is still `committed` is reported.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use contracts::ScrubError;
use sc_audit::{AuditEvent, AuditEventKind, AuditWriter, SubjectRef};
use sc_clock::Clock;
use sc_db::Db;
use tokio::fs;
use uuid::Uuid;

use super::scrub_repo;
use super::verify::Verdict;
use crate::error::{BlobIds, io_fault, run_fault};
use crate::path::blob_path;

pub(super) enum Reported {
    /// The blob is `committed` and its file is bad: counted, marked unavailable, one event for the first finding.
    Counted,
    /// A collector holds the file at this moment; nothing is reported and the blob is checked by the next run.
    Deferred,
    /// The row is gone or no longer `committed`; the finding belongs to the removal, not to corruption.
    NotCommitted,
}

pub(super) struct Reporter {
    pub db: Db,
    pub audit: AuditWriter,
    pub clock: Arc<dyn Clock>,
    pub root: PathBuf,
}

fn ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

fn parked_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".gc");
    PathBuf::from(name)
}

async fn exists(path: &Path) -> io::Result<bool> {
    match fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

impl Reporter {
    /// `finding` is `Corrupt` or `Missing`.
    pub async fn report(&self, blob_id: Uuid, finding: Verdict) -> Result<Reported, ScrubError> {
        if finding == Verdict::Missing && self.collector_holds(blob_id).await? {
            return Ok(Reported::Deferred);
        }
        let at = self.clock.now_utc().as_unix_micros();
        let marked = scrub_repo::recheck_and_mark(&self.db, blob_id, at)
            .await
            .map_err(|error| ScrubError::Unexpected(run_fault(error, ids(blob_id))))?;
        let Some(marked) = marked else {
            return Ok(Reported::NotCommitted);
        };
        if marked.first {
            self.audit
                .record_best_effort(finding_event(blob_id, marked.account, finding))
                .await;
        }
        Ok(Reported::Counted)
    }

    /// The parked name is looked at first and the blob's own name second. A file moves between the two by one
    /// rename, so a file that was back under its own name before the first look is seen by the second.
    async fn collector_holds(&self, blob_id: Uuid) -> Result<bool, ScrubError> {
        let own = blob_path(&self.root, blob_id);
        let fail = |error: io::Error| ScrubError::Unexpected(io_fault(&error, ids(blob_id)));
        let parked = exists(&parked_path(&own)).await.map_err(fail)?;
        Ok(parked || exists(&own).await.map_err(fail)?)
    }
}

fn finding_event(blob_id: Uuid, account: Uuid, finding: Verdict) -> AuditEvent {
    let reason = if finding == Verdict::Corrupt {
        "corrupt"
    } else {
        "missing"
    };
    AuditEvent {
        subject: Some(SubjectRef {
            subject_type: "blob",
            id: blob_id,
        }),
        detail: sc_audit::AuditDetail::new()
            .with_id("account", account)
            .with_code("reason", reason)
            .with_code("source", "scrub"),
        ..AuditEvent::new(AuditEventKind::ScrubMismatch)
    }
}
