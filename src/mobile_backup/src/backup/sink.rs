//! The results of a staged batch reach the checkpoint here, before `sync-core` clears the batch record.

use sync_core::{BatchOutcome, BatchReport, BatchSink, StoreError};
use sync_core_proto::EntryResult;
use tracing::{error, warn};

use crate::checkpoint::{Acknowledged, AssetStore};

impl BatchSink for AssetStore {
    fn on_batch(&self, report: BatchReport) -> Result<(), StoreError> {
        let BatchOutcome::Committed(result) = report.outcome else {
            warn!("batch id was reused, its assets stay pending");
            return Ok(());
        };
        let mut done = Vec::with_capacity(result.entries.len());
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
                EntryResult::EntryError { code } => {
                    warn!(
                        code,
                        "the server refused a batch entry, its asset stays pending"
                    );
                }
            }
        }
        self.mark_uploaded(&done).map_err(|cause| {
            error!(error = %cause, "batch results were not stored");
            StoreError
        })
    }
}
