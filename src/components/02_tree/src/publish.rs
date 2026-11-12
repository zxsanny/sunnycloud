//! `TreeCommit::publish`: one transaction in the order of the tree-commit contract. `AccountGuard::ensure_active`
//! first, the account advisory lock, every effect's `on_begin`, each entry in its own savepoint (node and revision
//! writes, the usage delta, the reservation release, every effect's `on_entry`), every effect's `on_batch`, and
//! `FeedStamp::stamp` over the union of touched nodes as the last statement; COMMIT, then `FeedNotifier::notify`.
//! With nothing touched there is no stamp and no notify.

use std::sync::Arc;

use contracts::{
    AccountGuardError, Begin, CommitActor, CommitEffect, EffectError, EntryFailure, EntryOutcome,
    EntryResult, FeedStampError, NodeKind, PublishEntry, PublishOutcome, PublishResult,
    PublishedEntry, QuotaLedgerError, Seq, StoredResponse, Target, TouchedNodes, TreeCommit,
    TreeCommitError, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx, Scope};
use sc_error::ErrorCode;
use sc_ids::new_id;
use sc_names::{name_key, validate_name};
use uuid::Uuid;

use crate::error::OutcomePending;
use crate::node_repo;
use crate::publish_repo::{self, NewFile};
use crate::retry::run_retrying_each;
use crate::service::PgTreeService;

/// The unique index that keeps names unique per folder; on commit a hit on it is a taken name.
const NAME_INDEX: &str = "nodes_parent_name_key";

enum Done {
    Replayed(StoredResponse),
    Published(Vec<EntryResult>, Option<Seq>),
}

/// The end of one entry: a typed failure rolls back its savepoint only; `Call` ends the whole call.
enum EntryError {
    Failed(EntryFailure),
    Call(TreeCommitError),
}

impl From<DbError> for EntryError {
    fn from(error: DbError) -> Self {
        EntryError::Call(TreeCommitError::Unexpected(Box::new(error)))
    }
}

/// A published entry before the effects see it: its outcome, node, revision and usage delta.
struct Written {
    outcome: PublishOutcome,
    node_id: Uuid,
    revision_id: Uuid,
    delta: i64,
}

#[async_trait]
impl TreeCommit for PgTreeService {
    async fn publish(
        &self,
        actor: CommitActor,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
        now: Timestamp,
    ) -> Result<PublishResult, TreeCommitError> {
        let account = actor.account_id;
        let (entries, effects) = (entries.as_slice(), effects.as_slice());
        let done = run_retrying_each(&self.db, Scope::Account(account), || {
            async move |tx: &mut DbTx| self.publish_in(tx, account, entries, effects, now).await
        })
        .await?;
        match done {
            Done::Replayed(response) => Ok(PublishResult::Replayed(response)),
            Done::Published(results, seq) => {
                if let Some(seq) = seq {
                    self.notifier.notify(account, seq);
                }
                Ok(PublishResult::Published(results))
            }
        }
    }
}

impl PgTreeService {
    async fn publish_in(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        entries: &[PublishEntry],
        effects: &[Arc<dyn CommitEffect>],
        now: Timestamp,
    ) -> Result<Done, TreeCommitError> {
        self.guard
            .ensure_active(tx, account)
            .await
            .map_err(|error| match error {
                AccountGuardError::AccountNotActive => TreeCommitError::AccountNotActive,
                AccountGuardError::Unexpected(fault) => TreeCommitError::Unexpected(fault),
            })?;
        tx.advisory_lock_account(account)
            .await
            .map_err(|error| TreeCommitError::Unexpected(Box::new(error)))?;
        for effect in effects {
            if let Begin::Replay(response) = effect.on_begin(tx).await.map_err(call_error)? {
                return Ok(Done::Replayed(response));
            }
        }
        let mut outcomes = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let result = match tx
                .savepoint(async |tx| self.publish_entry(tx, account, entry, effects, now).await)
                .await
            {
                Ok(result) => result,
                Err(EntryError::Failed(failure)) => EntryResult::Failed(failure),
                Err(EntryError::Call(error)) => return Err(error),
            };
            outcomes.push(EntryOutcome { index, result });
        }
        for effect in effects {
            effect.on_batch(tx, &outcomes).await.map_err(call_error)?;
        }
        let seq = match touched_union(&outcomes) {
            Some(touched) => Some(self.stamp.stamp(tx, account, touched).await.map_err(
                |error| match error {
                    FeedStampError::Unexpected(fault) => TreeCommitError::Unexpected(fault),
                    missing @ FeedStampError::AccountFeedMissing => {
                        TreeCommitError::Unexpected(Box::new(missing))
                    }
                },
            )?),
            None => None,
        };
        let results = outcomes.into_iter().map(|outcome| outcome.result).collect();
        Ok(Done::Published(results, seq))
    }

    async fn publish_entry(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        entry: &PublishEntry,
        effects: &[Arc<dyn CommitEffect>],
        now: Timestamp,
    ) -> Result<EntryResult, EntryError> {
        let written = match &entry.target {
            Target::New { parent_id, name } => {
                self.create_file(tx, account, *parent_id, name, entry, now)
                    .await?
            }
            Target::Existing {
                node_id,
                base_revision,
            } => {
                self.add_revision(tx, account, *node_id, *base_revision, entry, now)
                    .await?
            }
        };
        if written.delta != 0 {
            self.quota
                .adjust_used(tx, account, written.delta)
                .await
                .map_err(quota_error)?;
        }
        if entry.revision.reserved_bytes != 0 {
            self.quota
                .release_reservation(tx, account, entry.revision.reserved_bytes)
                .await
                .map_err(quota_error)?;
        }
        let published = PublishedEntry {
            node_id: written.node_id,
            revision_id: written.revision_id,
            content_hash: entry.revision.content_hash,
            media_type: entry.revision.media_type.clone(),
            size: entry.revision.size,
            import_hint: entry.revision.import_hint,
            vault: entry.vault,
        };
        for effect in effects {
            effect
                .on_entry(tx, &published)
                .await
                .map_err(|error| match error {
                    EffectError::Entry(code) => {
                        EntryError::Failed(EntryFailure::EffectFailed(code))
                    }
                    batch @ EffectError::Batch(_) => EntryError::Call(call_error(batch)),
                })?;
        }
        Ok(EntryResult::Published {
            outcome: written.outcome,
            node_id: written.node_id,
            revision_id: written.revision_id,
            conflict_copy: None,
        })
    }

    /// `Created`. The parent row is locked `FOR SHARE`, so nothing is added under a folder whose tombstone
    /// committed first.
    async fn create_file(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
        entry: &PublishEntry,
        now: Timestamp,
    ) -> Result<Written, EntryError> {
        let name = validate_name(name)
            .map_err(|reason| EntryError::Failed(EntryFailure::NameInvalid(reason.to_string())))?;
        let failure = match node_repo::lock_for_share(tx, account, parent_id).await? {
            None => Some(EntryFailure::ParentNotFound),
            Some(parent) if parent.kind != NodeKind::Folder => Some(EntryFailure::NotAFolder),
            Some(parent) if parent.deleted => Some(EntryFailure::ParentDeleted),
            Some(_) => None,
        };
        if let Some(failure) = failure {
            return Err(EntryError::Failed(failure));
        }
        let key = name_key(&name);
        let file = NewFile {
            id: new_id(self.clock.as_ref()),
            account,
            parent_id,
            name: name.as_str(),
            name_key: key.as_str(),
            revision_id: entry.revision.revision_id,
            now,
        };
        publish_repo::insert_file(tx, &file)
            .await
            .map_err(|error| match error {
                DbError::UniqueViolation {
                    constraint: Some(ref constraint),
                } if constraint == NAME_INDEX => pending("CreatedAsCopy"),
                other => EntryError::from(other),
            })?;
        insert_revision(tx, account, file.id, entry, now).await?;
        Ok(Written {
            outcome: PublishOutcome::Created,
            node_id: file.id,
            revision_id: entry.revision.revision_id,
            delta: entry.revision.size,
        })
    }

    /// `Updated` (base is the current revision), `Replaced` (no base) or `NoChange` (same content as the current
    /// revision, whatever the base). The usage delta of an update is AZ-3711's; a stale base and a tombstoned file
    /// are AZ-3710's outcomes.
    async fn add_revision(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        node_id: Uuid,
        base_revision: Option<Uuid>,
        entry: &PublishEntry,
        now: Timestamp,
    ) -> Result<Written, EntryError> {
        let file = publish_repo::lock_file(tx, account, node_id)
            .await?
            .filter(|file| file.kind == NodeKind::File)
            .ok_or(EntryError::Failed(EntryFailure::NotFound))?;
        if file.deleted {
            return Err(pending("Revived"));
        }
        let same_content =
            file.current_hash.as_deref() == Some(entry.revision.content_hash.as_bytes().as_slice());
        if let (true, Some(existing_revision)) = (same_content, file.current_revision_id) {
            return Ok(Written {
                outcome: PublishOutcome::NoChange { existing_revision },
                node_id,
                revision_id: existing_revision,
                delta: 0,
            });
        }
        let outcome = match base_revision {
            None => PublishOutcome::Replaced,
            Some(base) if Some(base) == file.current_revision_id => PublishOutcome::Updated,
            Some(_) => return Err(pending("ConflictCopied")),
        };
        insert_revision(tx, account, node_id, entry, now).await?;
        publish_repo::set_current(tx, account, node_id, entry.revision.revision_id, now).await?;
        Ok(Written {
            outcome,
            node_id,
            revision_id: entry.revision.revision_id,
            delta: 0,
        })
    }
}

/// `revisions.blob_id` references `blobs (id, account_id)`, so an unknown blob and another account's blob fail
/// the same foreign key and give the same entry failure.
async fn insert_revision(
    tx: &mut DbTx,
    account: Uuid,
    node: Uuid,
    entry: &PublishEntry,
    now: Timestamp,
) -> Result<(), EntryError> {
    publish_repo::insert_revision(tx, account, node, &entry.revision, now)
        .await
        .map_err(|error| match error {
            DbError::ForeignKeyViolation => EntryError::Failed(EntryFailure::BlocklistMismatch),
            other => EntryError::from(other),
        })
}

/// The nodes the call changed, each once, in entry order; `None` when nothing was touched (no stamp).
fn touched_union(outcomes: &[EntryOutcome]) -> Option<TouchedNodes> {
    let mut touched: Vec<Uuid> = Vec::new();
    for outcome in outcomes {
        let EntryResult::Published {
            outcome, node_id, ..
        } = &outcome.result
        else {
            continue;
        };
        let nodes = match *outcome {
            PublishOutcome::NoChange { .. } => [None, None],
            PublishOutcome::ConflictCopied { copy_node } => [Some(*node_id), Some(copy_node)],
            PublishOutcome::Created
            | PublishOutcome::Updated
            | PublishOutcome::Replaced
            | PublishOutcome::CreatedAsCopy { .. }
            | PublishOutcome::Revived => [Some(*node_id), None],
        };
        for node in nodes.into_iter().flatten() {
            if !touched.contains(&node) {
                touched.push(node);
            }
        }
    }
    (!touched.is_empty()).then_some(TouchedNodes(touched))
}

/// An effect error that ends the whole call; a receipt effect reports a reused commit id with its code.
fn call_error(error: EffectError) -> TreeCommitError {
    match error {
        EffectError::Entry(ErrorCode::CommitIdReused)
        | EffectError::Batch(ErrorCode::CommitIdReused) => TreeCommitError::CommitIdReused,
        EffectError::Entry(code) | EffectError::Batch(code) => TreeCommitError::EffectFailed(code),
    }
}

fn quota_error(error: QuotaLedgerError) -> EntryError {
    EntryError::Call(match error {
        QuotaLedgerError::Unexpected(fault) => TreeCommitError::Unexpected(fault),
        other => TreeCommitError::Unexpected(Box::new(other)),
    })
}

fn pending(outcome: &'static str) -> EntryError {
    EntryError::Call(TreeCommitError::Unexpected(Box::new(OutcomePending(
        outcome,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn published(outcome: PublishOutcome, node_id: Uuid) -> EntryOutcome {
        EntryOutcome {
            index: 0,
            result: EntryResult::Published {
                outcome,
                node_id,
                revision_id: Uuid::new_v4(),
                conflict_copy: None,
            },
        }
    }

    #[test]
    fn the_union_holds_each_touched_node_once_in_entry_order() {
        // Arrange
        let (a, b, copy) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let outcomes = [
            published(PublishOutcome::Created, a),
            published(PublishOutcome::Updated, b),
            EntryOutcome {
                index: 2,
                result: EntryResult::Failed(EntryFailure::ParentDeleted),
            },
            published(PublishOutcome::Replaced, a),
            published(PublishOutcome::ConflictCopied { copy_node: copy }, b),
        ];

        // Act
        let touched = touched_union(&outcomes);

        // Assert
        assert_eq!(touched, Some(TouchedNodes(vec![a, b, copy])));
    }

    #[test]
    fn no_change_and_failed_entries_touch_nothing() {
        // Arrange
        let node = Uuid::new_v4();
        let outcomes = [
            published(
                PublishOutcome::NoChange {
                    existing_revision: Uuid::new_v4(),
                },
                node,
            ),
            EntryOutcome {
                index: 1,
                result: EntryResult::Failed(EntryFailure::NameInvalid("empty".into())),
            },
        ];

        // Act
        let touched = touched_union(&outcomes);

        // Assert
        assert_eq!(touched, None);
    }

    #[test]
    fn a_reused_commit_id_is_its_own_whole_call_error() {
        // Arrange
        let errors = [
            EffectError::Batch(ErrorCode::CommitIdReused),
            EffectError::Batch(ErrorCode::UploadSessionGone),
        ];

        // Act
        let mapped = errors.map(call_error);

        // Assert
        assert!(matches!(mapped[0], TreeCommitError::CommitIdReused));
        assert!(matches!(
            mapped[1],
            TreeCommitError::EffectFailed(ErrorCode::UploadSessionGone)
        ));
    }
}
