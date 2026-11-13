//! `TreeCommit::publish`: one transaction in the order of the tree-commit contract. `AccountGuard::ensure_active`
//! first, the account advisory lock, every effect's `on_begin`, each entry in its own savepoint (node and revision
//! writes, the usage delta, the reservation release, every effect's `on_entry`), every effect's `on_batch`, and
//! `FeedStamp::stamp` over the union of touched nodes as the last statement; COMMIT, then `FeedNotifier::notify`.
//! With nothing touched there is no stamp and no notify. The conflict outcomes (`ConflictCopied`, `CreatedAsCopy`,
//! `Revived`) are in `crate::conflict`.

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

use crate::conflict::{Naming, insert_as_copy};
use crate::conflict_repo;
use crate::node_repo;
use crate::publish_repo::{self, NewFile};
use crate::retry::run_retrying_each;
use crate::service::PgTreeService;

enum Done {
    Replayed(StoredResponse),
    Published(Vec<EntryResult>, Option<Seq>),
}

/// The end of one entry: a typed failure rolls back its savepoint only; `Call` ends the whole call.
pub(crate) enum EntryError {
    Failed(EntryFailure),
    Call(TreeCommitError),
}

impl From<DbError> for EntryError {
    fn from(error: DbError) -> Self {
        EntryError::Call(TreeCommitError::Unexpected(Box::new(error)))
    }
}

/// A published entry before the effects see it: its outcome, node, revision, usage delta, and the ancestors a
/// revival brought back (touched besides the outcome's own nodes).
pub(crate) struct Written {
    pub outcome: PublishOutcome,
    pub node_id: Uuid,
    pub revision_id: Uuid,
    pub delta: i64,
    pub revived: Vec<Uuid>,
}

/// What every step of one entry needs: the account, the entry, the inputs of conflict-copy names and the commit
/// time of the rows.
pub(crate) struct Committing<'a> {
    pub account: Uuid,
    pub entry: &'a PublishEntry,
    pub naming: &'a Naming<'a>,
    pub now: Timestamp,
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
        let (actor, entries, effects) = (&actor, entries.as_slice(), effects.as_slice());
        let done = run_retrying_each(&self.db, Scope::Account(account), || {
            async move |tx: &mut DbTx| self.publish_in(tx, actor, entries, effects, now).await
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
        actor: &CommitActor,
        entries: &[PublishEntry],
        effects: &[Arc<dyn CommitEffect>],
        now: Timestamp,
    ) -> Result<Done, TreeCommitError> {
        let account = actor.account_id;
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
        let naming = Naming {
            at: self.clock.now_utc(),
            device_label: &actor.device_label,
        };
        let mut outcomes = Vec::with_capacity(entries.len());
        let mut revived = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let at = Committing {
                account,
                entry,
                naming: &naming,
                now,
            };
            let result = match tx
                .savepoint(async |tx| self.publish_entry(tx, &at, effects).await)
                .await
            {
                Ok((result, ancestors)) => {
                    revived.extend(ancestors);
                    result
                }
                Err(EntryError::Failed(failure)) => EntryResult::Failed(failure),
                Err(EntryError::Call(error)) => return Err(error),
            };
            outcomes.push(EntryOutcome { index, result });
        }
        for effect in effects {
            effect.on_batch(tx, &outcomes).await.map_err(call_error)?;
        }
        let seq = match touched_union(&outcomes, &revived) {
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

    /// The entry's result and the ancestors it revived.
    async fn publish_entry(
        &self,
        tx: &mut DbTx,
        at: &Committing<'_>,
        effects: &[Arc<dyn CommitEffect>],
    ) -> Result<(EntryResult, Vec<Uuid>), EntryError> {
        let (account, entry) = (at.account, at.entry);
        let written = match &entry.target {
            Target::New { parent_id, name } => self.create_file(tx, at, *parent_id, name).await?,
            Target::Existing {
                node_id,
                base_revision,
            } => self.add_revision(tx, at, *node_id, *base_revision).await?,
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
        // 03 reads `conflict_copy` as the node holding the older content, or the committed node itself when that
        // node got a conflict-copy name (`commit_service.md`, G-03-3).
        let conflict_copy = match written.outcome {
            PublishOutcome::ConflictCopied { copy_node } => Some(copy_node),
            PublishOutcome::CreatedAsCopy { node } => Some(node),
            _ => None,
        };
        let result = EntryResult::Published {
            outcome: written.outcome,
            node_id: written.node_id,
            revision_id: written.revision_id,
            conflict_copy,
        };
        Ok((result, written.revived))
    }

    /// `Created`, or `CreatedAsCopy` under a conflict-copy name when the name is taken: never a name conflict.
    /// The parent row is locked `FOR SHARE`, so nothing is added under a folder whose tombstone committed first.
    async fn create_file(
        &self,
        tx: &mut DbTx,
        at: &Committing<'_>,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Written, EntryError> {
        let (account, entry, now) = (at.account, at.entry, at.now);
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
        let outcome = if publish_repo::insert_file(tx, &file).await? {
            PublishOutcome::Created
        } else {
            insert_as_copy(tx, &file, at.naming).await?;
            PublishOutcome::CreatedAsCopy { node: file.id }
        };
        insert_revision(tx, account, file.id, entry, now).await?;
        Ok(Written {
            outcome,
            node_id: file.id,
            revision_id: entry.revision.revision_id,
            delta: entry.revision.size,
            revived: Vec::new(),
        })
    }

    /// `Revived` when the file or a folder above it is tombstoned (edit beats delete, whatever the base), else
    /// `NoChange` (same content as the current revision, whatever the base), `Replaced` (no base), `Updated` (base
    /// is the current revision) or `ConflictCopied` (stale base). The usage deltas of these are AZ-3711's.
    async fn add_revision(
        &self,
        tx: &mut DbTx,
        at: &Committing<'_>,
        node_id: Uuid,
        base_revision: Option<Uuid>,
    ) -> Result<Written, EntryError> {
        let (account, entry, now) = (at.account, at.entry, at.now);
        let file = publish_repo::lock_file(tx, account, node_id)
            .await?
            .filter(|file| file.kind == NodeKind::File)
            .ok_or(EntryError::Failed(EntryFailure::NotFound))?;
        let ancestors = match file.parent_id {
            Some(parent) => conflict_repo::tombstoned_ancestors(tx, account, parent).await?,
            None => Vec::new(),
        };
        if file.deleted || !ancestors.is_empty() {
            return self.revive(tx, at, node_id, &file, ancestors).await;
        }
        let same_content =
            file.current_hash.as_deref() == Some(entry.revision.content_hash.as_bytes().as_slice());
        if let (true, Some(existing_revision)) = (same_content, file.current_revision_id) {
            return Ok(Written {
                outcome: PublishOutcome::NoChange { existing_revision },
                node_id,
                revision_id: existing_revision,
                delta: 0,
                revived: Vec::new(),
            });
        }
        // A live file always has a current revision; without one there is nothing a stale base could supersede.
        let outcome = match (base_revision, file.current_revision_id) {
            (Some(base), Some(current)) if base == current => PublishOutcome::Updated,
            (Some(_), Some(superseded)) => {
                return self
                    .keep_superseded(tx, at, node_id, &file, superseded)
                    .await;
            }
            (None, _) | (Some(_), None) => PublishOutcome::Replaced,
        };
        insert_revision(tx, account, node_id, entry, now).await?;
        publish_repo::set_current(tx, account, node_id, entry.revision.revision_id, now).await?;
        Ok(Written {
            outcome,
            node_id,
            revision_id: entry.revision.revision_id,
            delta: 0,
            revived: Vec::new(),
        })
    }
}

/// `revisions.blob_id` references `blobs (id, account_id)`, so an unknown blob and another account's blob fail
/// the same foreign key and give the same entry failure.
pub(crate) async fn insert_revision(
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

/// The nodes the call changed, each once, in entry order, then the revived ancestors; `None` when nothing was
/// touched (no stamp).
fn touched_union(outcomes: &[EntryOutcome], revived: &[Uuid]) -> Option<TouchedNodes> {
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
    for node in revived {
        if !touched.contains(node) {
            touched.push(*node);
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
        let touched = touched_union(&outcomes, &[]);

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
        let touched = touched_union(&outcomes, &[]);

        // Assert
        assert_eq!(touched, None);
    }

    #[test]
    fn revived_ancestors_join_the_union_once_after_the_entries() {
        // Arrange
        let (file, folder, upper) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let outcomes = [published(PublishOutcome::Revived, file)];

        // Act
        let touched = touched_union(&outcomes, &[upper, folder, upper]);

        // Assert
        assert_eq!(touched, Some(TouchedNodes(vec![file, upper, folder])));
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
