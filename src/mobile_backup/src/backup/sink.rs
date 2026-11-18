//! The results of a staged batch reach the checkpoint here, before `sync-core` clears the batch record.

use sc_error::ErrorCode;
use sync_core::{BatchOutcome, BatchReport, BatchSink, StoreError};
use sync_core_proto::EntryResult;
use tracing::{error, warn};

use super::verdict::record_error;
use crate::checkpoint::{Acknowledged, AssetEnd, AssetStore};

/// The code is the server's text: only a code of the shared table is repeated in a log line or `last_error`.
fn known(code: &str) -> &'static str {
    ErrorCode::from_wire(code).map_or("unknown", ErrorCode::as_str)
}

/// How a refused entry ends its asset. `None`: the entry is retried by the next run (its staged row is gone) or
/// belongs to the folder recovery of E-27/c13, so the asset stays pending. Any other code is not a state of the
/// contract's table, so the asset ends `failed`, as the table's last row says.
fn refusal(code: &str) -> Option<AssetEnd> {
    match ErrorCode::from_wire(code) {
        Some(ErrorCode::InvalidName) => Some(AssetEnd::Name),
        Some(ErrorCode::SizeTooLarge) => Some(AssetEnd::Size),
        Some(
            ErrorCode::UploadSessionGone | ErrorCode::ParentNotFound | ErrorCode::ParentDeleted,
        ) => None,
        _ => Some(AssetEnd::Failed),
    }
}

impl BatchSink for AssetStore {
    fn on_batch(&self, report: BatchReport) -> Result<(), StoreError> {
        let BatchOutcome::Committed(result) = report.outcome else {
            warn!("batch id was reused, its assets stay pending");
            return Ok(());
        };
        let mut done = Vec::with_capacity(result.entries.len());
        let mut ended = Vec::new();
        for (entry, result) in report.entries.iter().zip(result.entries) {
            match result {
                EntryResult::Committed {
                    node_id,
                    revision_id,
                    conflict_copy,
                } => done.push(Acknowledged {
                    client_key: entry.staged_key,
                    node_id: Some(conflict_copy.unwrap_or(node_id)),
                    revision_id: Some(revision_id),
                }),
                EntryResult::EntryError { code } => match refusal(&code) {
                    Some(end) => {
                        if end == AssetEnd::Failed {
                            note_refusal(self, &code)?;
                        }
                        ended.push((entry.staged_key, end));
                    }
                    None => warn!(
                        code = known(&code),
                        "a batch entry was refused, its asset stays pending"
                    ),
                },
            }
        }
        let stored = self
            .mark_uploaded(&done)
            .and_then(|()| self.mark_ended_by_key(&ended));
        stored.map_err(|cause| {
            error!(error = %cause, "batch results were not stored");
            StoreError
        })
    }
}

fn note_refusal(assets: &AssetStore, code: &str) -> Result<(), StoreError> {
    record_error(
        assets,
        format!("batch entry refused ({})", known(code)),
        None,
    )
    .map_err(|cause| {
        error!(error = %cause, "the refusal of a batch entry was not recorded");
        StoreError
    })
}
