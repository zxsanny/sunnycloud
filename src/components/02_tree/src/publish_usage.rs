//! The usage side of one `publish` entry (AZ-3711, AC-74): the exact delta of its outcome through
//! `QuotaLedger::adjust_used`, the release of its reservation, and the blob of the revision that stops counting
//! queued `replaced` after retention. It runs inside the entry's savepoint, so a failed entry undoes all three.

use contracts::{
    Due, GcQueueError, GcReason, PublishEntry, PublishOutcome, QuotaLedgerError, TreeCommitError,
};
use sc_db::DbTx;
use uuid::Uuid;

use crate::node_repo::CurrentContent;
use crate::publish::EntryError;
use crate::service::PgTreeService;

/// What one entry does to the account's usage: the delta, and the blob of the revision that stopped counting.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Usage {
    pub delta: i64,
    pub replaced_blob: Option<Uuid>,
}

/// Usage counts the current revision of every file whose own node is live (`LiveUsageSource::live_bytes`).
/// `counted` is the revision of the entry's file that counted before the entry: none for a new file or a
/// tombstoned one. `NoChange` changes nothing; `ConflictCopied` adds the new revision and keeps counting the
/// superseded one in its copy; every other outcome makes the new revision count instead of `counted`.
pub(crate) fn usage(
    outcome: &PublishOutcome,
    new_size: i64,
    counted: Option<&CurrentContent>,
) -> Usage {
    match (outcome, counted) {
        (PublishOutcome::NoChange { .. }, _) => Usage {
            delta: 0,
            replaced_blob: None,
        },
        (PublishOutcome::ConflictCopied { .. }, _) | (_, None) => Usage {
            delta: new_size,
            replaced_blob: None,
        },
        (_, Some(old)) => Usage {
            // Both sizes are non-negative (`revisions_size_check`), so the difference cannot overflow.
            delta: new_size - old.size,
            replaced_blob: Some(old.blob_id),
        },
    }
}

impl PgTreeService {
    /// The delta, the reservation release and the queue write of one entry, in that order (tree-commit step 4).
    pub(crate) async fn apply_usage(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        entry: &PublishEntry,
        usage: &Usage,
    ) -> Result<(), EntryError> {
        if usage.delta != 0 {
            self.quota
                .adjust_used(tx, account, usage.delta)
                .await
                .map_err(quota_error)?;
        }
        if entry.revision.reserved_bytes != 0 {
            self.quota
                .release_reservation(tx, account, entry.revision.reserved_bytes)
                .await
                .map_err(quota_error)?;
        }
        if let Some(blob_id) = usage.replaced_blob {
            self.gc
                .schedule_in_tx(tx, blob_id, GcReason::Replaced, Due::AfterRetention)
                .await
                .map_err(|GcQueueError::Unexpected(fault)| {
                    EntryError::Call(TreeCommitError::Unexpected(fault))
                })?;
        }
        Ok(())
    }
}

fn quota_error(error: QuotaLedgerError) -> EntryError {
    EntryError::Call(match error {
        QuotaLedgerError::Unexpected(fault) => TreeCommitError::Unexpected(fault),
        other => TreeCommitError::Unexpected(Box::new(other)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: i64 = 700;

    fn old() -> CurrentContent {
        CurrentContent {
            blob_id: Uuid::from_u128(9),
            size: 300,
        }
    }

    #[test]
    fn each_outcome_applies_its_delta_from_the_tree_commit_table() {
        // Arrange
        let old = old();
        let copy = PublishOutcome::ConflictCopied {
            copy_node: Uuid::from_u128(1),
        };
        let same = PublishOutcome::NoChange {
            existing_revision: Uuid::from_u128(2),
        };
        let replaced = Some(old.blob_id);

        // Act
        let deltas = [
            usage(&PublishOutcome::Created, NEW, None),
            usage(&PublishOutcome::Updated, NEW, Some(&old)),
            usage(&PublishOutcome::Replaced, NEW, Some(&old)),
            usage(&copy, NEW, Some(&old)),
            usage(&PublishOutcome::Revived, NEW, None),
            usage(&same, NEW, Some(&old)),
        ];

        // Assert
        let expected = [
            (NEW, None),
            (NEW - 300, replaced),
            (NEW - 300, replaced),
            (NEW, None),
            (NEW, None),
            (0, None),
        ];
        let got: Vec<(i64, Option<Uuid>)> = deltas
            .into_iter()
            .map(|usage| (usage.delta, usage.replaced_blob))
            .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn a_revived_file_that_still_counted_replaces_its_old_revision() {
        // Arrange: a live file under a tombstoned folder the cascade has not marked yet.
        let old = old();

        // Act
        let revived = usage(&PublishOutcome::Revived, NEW, Some(&old));

        // Assert
        assert_eq!(
            revived,
            Usage {
                delta: NEW - 300,
                replaced_blob: Some(old.blob_id),
            }
        );
    }
}
