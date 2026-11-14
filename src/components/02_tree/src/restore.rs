//! `TreeRestore::restore_revision` (`admin restore`, AC-30, decision 4). One system-scope transaction under the
//! account advisory lock: the target node (by id, or by a path whose every part is the live node of that name,
//! else the most recently tombstoned one), the latest revision with `created_at <= at`,
//! `BlobRegistry::reinstate_in_tx` for its blob, then the existing revision row is re-pointed (never a second
//! row: `revisions.blob_id` is unique): `Original` when the node's folder and every folder above it are live,
//! else `Restored`. The usage delta, `FeedStamp::stamp` last, COMMIT, then `FeedNotifier::notify` (from the CLI
//! process it reaches nobody; held polls see the change on 05's recheck). Any refusal rolls everything back.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    BlobRegistry, BlobRegistryError, Due, EntryFailure, FeedStampError, GcQueueError, GcReason,
    NodeKind, NodeOrPath, Placement, QuotaLedgerError, ReinstateOutcome, RestoreError,
    RestoreOutcome, Seq, TouchedNodes, TreeCommitError, TreeRestore, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::new_id;
use sc_names::{name_key, split_path, validate_name};
use thiserror::Error;
use uuid::Uuid;

use crate::conflict::{Naming, insert_as_copy, revive_node};
use crate::conflict_repo::{self, Tombstoned};
use crate::maintenance_repo::in_system;
use crate::node_repo::{self, NewFolder};
use crate::publish::EntryError;
use crate::publish_repo::{self, FileRow, NewFile};
use crate::reader_repo;
use crate::restore_repo::{self, Picked};
use crate::retry::retry_aborted;
use crate::service::PgTreeService;

/// The root folder a file lands in when its own folder is gone (decision 4).
pub const RESTORED_FOLDER: &str = "Restored";

/// No device commits a restore, so its conflict-copy names carry no device label (helper 06 drops the part).
const NO_DEVICE: &str = "";

#[derive(Debug, Error)]
enum RestoreFault {
    #[error("tree row breaks an invariant: {0}")]
    BrokenRow(&'static str),
    #[error("the root name {RESTORED_FOLDER:?} is held by a file")]
    RestoredHeldByFile,
    #[error("unexpected entry failure in a restore: {0:?}")]
    EntryFailed(EntryFailure),
}

/// The restore of the operator CLI. It reuses the service's database, quota ledger, queue, stamp, notifier and
/// clock, and adds the blob registry (reinstate) that no request-path operation needs.
pub struct PgTreeRestore {
    tree: Arc<PgTreeService>,
    registry: Arc<dyn BlobRegistry>,
    retention: Duration,
}

impl PgTreeRestore {
    /// `retention` is the blob retention (the blobstore `retention` key): an instant before now minus it is
    /// `BeyondRetention` without a transaction.
    pub fn new(
        tree: Arc<PgTreeService>,
        registry: Arc<dyn BlobRegistry>,
        retention: Duration,
    ) -> Self {
        PgTreeRestore {
            tree,
            registry,
            retention,
        }
    }

    async fn restore_in(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        target: &NodeOrPath,
        at: Timestamp,
        now: Timestamp,
    ) -> Result<(RestoreOutcome, Option<Seq>), RestoreError> {
        tx.advisory_lock_account(account)
            .await
            .map_err(unexpected)?;
        let node_id = match target {
            NodeOrPath::Node(id) => *id,
            NodeOrPath::Path(path) => resolve(tx, account, path).await?,
        };
        let file = publish_repo::lock_file(tx, account, node_id)
            .await
            .map_err(unexpected)?
            .filter(|file| file.kind == NodeKind::File)
            .ok_or(RestoreError::NotFound)?;
        let picked = restore_repo::revision_at(tx, account, node_id, at)
            .await
            .map_err(unexpected)?
            .ok_or(RestoreError::NotFound)?;
        let parent = file.parent_id.ok_or(broken("a file without a parent"))?;
        let folder_live = conflict_repo::tombstoned_ancestors(tx, account, parent)
            .await
            .map_err(unexpected)?
            .is_empty();
        let is_current = file.current_revision_id == Some(picked.id);
        let outcome = RestoreOutcome {
            restored_revision: picked.id,
            placement: if folder_live {
                Placement::Original
            } else {
                Placement::Restored
            },
        };
        if folder_live && !file.deleted && is_current {
            return Ok((outcome, None));
        }
        self.reinstate(tx, account, picked.blob_id).await?;
        let naming = Naming {
            at: now,
            device_label: NO_DEVICE,
        };
        let restore = Restoring {
            account,
            node_id,
            parent,
            file: &file,
            picked: &picked,
            naming: &naming,
            now,
        };
        let (touched, delta) = if folder_live {
            self.place_original(tx, &restore).await?
        } else {
            self.place_restored(tx, &restore).await?
        };
        if delta != 0 {
            self.tree
                .quota
                .adjust_used(tx, account, delta)
                .await
                .map_err(|error| match error {
                    QuotaLedgerError::Unexpected(fault) => RestoreError::Unexpected(fault),
                    other => RestoreError::Unexpected(Box::new(other)),
                })?;
        }
        let seq = self
            .tree
            .stamp
            .stamp(tx, account, TouchedNodes(touched))
            .await
            .map_err(|error| match error {
                FeedStampError::Unexpected(fault) => RestoreError::Unexpected(fault),
                missing @ FeedStampError::AccountFeedMissing => {
                    RestoreError::Unexpected(Box::new(missing))
                }
            })?;
        Ok((outcome, Some(seq)))
    }

    /// The blob back out of the GC queue; 04's refusals end the restore and roll it back.
    async fn reinstate(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<(), RestoreError> {
        match self
            .registry
            .reinstate_in_tx(tx, account, blob_id)
            .await
            .map_err(|BlobRegistryError::Unexpected(fault)| RestoreError::Unexpected(fault))?
        {
            ReinstateOutcome::Reinstated => Ok(()),
            ReinstateOutcome::BeyondRetention => Err(RestoreError::BeyondRetention),
            ReinstateOutcome::FileMissing => Err(RestoreError::FileMissing),
        }
    }

    /// The node (revived if tombstoned, under a conflict-copy name when its name is taken) gets the revision as
    /// current; the revision it replaces is queued `replaced`. Usage: +restored, minus what counted before.
    async fn place_original(
        &self,
        tx: &mut DbTx,
        at: &Restoring<'_>,
    ) -> Result<(Vec<Uuid>, i64), RestoreError> {
        let (account, file, picked) = (at.account, at.file, at.picked);
        if file.deleted {
            let own = Tombstoned {
                id: at.node_id,
                parent_id: at.parent,
                name: file.name.clone(),
            };
            revive_node(tx, account, &own, at.naming, at.now)
                .await
                .map_err(entry_error)?;
        }
        let mut delta = picked.size;
        if file.current_revision_id != Some(picked.id) {
            publish_repo::set_current(tx, account, at.node_id, picked.id, at.now)
                .await
                .map_err(unexpected)?;
            if let Some(replaced) = file.current {
                self.tree
                    .gc
                    .schedule_in_tx(
                        tx,
                        replaced.blob_id,
                        GcReason::Replaced,
                        Due::AfterRetention,
                    )
                    .await
                    .map_err(|GcQueueError::Unexpected(fault)| RestoreError::Unexpected(fault))?;
            }
            delta -= file.counted().map_or(0, |counted| counted.size);
        }
        Ok((vec![at.node_id], delta))
    }

    /// A new file node in the root folder `Restored` (created on first use) takes the revision row; the original
    /// node stops pointing at it. Usage: +restored, minus the original's size when it still counted it.
    async fn place_restored(
        &self,
        tx: &mut DbTx,
        at: &Restoring<'_>,
    ) -> Result<(Vec<Uuid>, i64), RestoreError> {
        let (account, file, picked) = (at.account, at.file, at.picked);
        let mut touched = Vec::with_capacity(2);
        let folder = match self.restored_folder(tx, account, at.now).await? {
            (folder, true) => {
                touched.push(folder);
                folder
            }
            (folder, false) => folder,
        };
        let name = validate_name(&file.name).map_err(|_| broken("a stored name is invalid"))?;
        let key = name_key(&name);
        let node = NewFile {
            id: new_id(self.tree.clock.as_ref()),
            account,
            parent_id: folder,
            // A vault file stays a vault file wherever it is restored: no media job, no share (E-30/c01).
            vault_id: file.vault_id,
            name: name.as_str(),
            name_key: key.as_str(),
            revision_id: picked.id,
            now: at.now,
        };
        if !publish_repo::insert_file(tx, &node)
            .await
            .map_err(unexpected)?
        {
            insert_as_copy(tx, &node, at.naming)
                .await
                .map_err(entry_error)?;
        }
        conflict_repo::reparent_revision(tx, account, picked.id, node.id)
            .await
            .map_err(unexpected)?;
        let mut delta = picked.size;
        if file.current_revision_id == Some(picked.id) {
            restore_repo::clear_current(tx, account, at.node_id, picked.id)
                .await
                .map_err(unexpected)?;
            delta -= file.counted().map_or(0, |counted| counted.size);
        }
        touched.push(node.id);
        Ok((touched, delta))
    }

    /// The live root folder `Restored`, created when absent: `(id, created)`.
    async fn restored_folder(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        now: Timestamp,
    ) -> Result<(Uuid, bool), RestoreError> {
        let root = reader_repo::root(tx, account)
            .await
            .map_err(unexpected)?
            .ok_or(broken("a file in an account without a root"))?;
        let name = validate_name(RESTORED_FOLDER).map_err(|_| broken("the folder name"))?;
        let key = name_key(&name);
        match reader_repo::live_child(tx, account, root, key.as_str())
            .await
            .map_err(unexpected)?
        {
            Some((id, NodeKind::Folder)) => Ok((id, false)),
            Some((_, NodeKind::File)) => Err(RestoreError::Unexpected(Box::new(
                RestoreFault::RestoredHeldByFile,
            ))),
            None => {
                let id = new_id(self.tree.clock.as_ref());
                let folder = NewFolder {
                    id,
                    account,
                    parent_id: root,
                    // The account's root folder is never a vault root, so `Restored` is in no vault.
                    vault_id: None,
                    name: name.as_str(),
                    name_key: key.as_str(),
                    now,
                };
                node_repo::insert_folder(tx, &folder)
                    .await
                    .map_err(unexpected)?;
                Ok((id, true))
            }
        }
    }
}

#[async_trait]
impl TreeRestore for PgTreeRestore {
    async fn restore_revision(
        &self,
        account: Uuid,
        target: NodeOrPath,
        at: Timestamp,
    ) -> Result<RestoreOutcome, RestoreError> {
        let now = self.tree.clock.now_utc();
        if beyond_retention(now, self.retention, at) {
            return Err(RestoreError::BeyondRetention);
        }
        let target = &target;
        let (outcome, seq) = retry_aborted(|| {
            in_system(&self.tree.db, "tree_restore", async move |tx: &mut DbTx| {
                self.restore_in(tx, account, target, at, now).await
            })
        })
        .await?;
        if let Some(seq) = seq {
            self.tree.notifier.notify(account, seq);
        }
        Ok(outcome)
    }
}

/// What both placements need.
struct Restoring<'a> {
    account: Uuid,
    node_id: Uuid,
    parent: Uuid,
    file: &'a FileRow,
    picked: &'a Picked,
    naming: &'a Naming<'a>,
    now: Timestamp,
}

/// `at` lies before the oldest instant the retention still covers.
fn beyond_retention(now: Timestamp, retention: Duration, at: Timestamp) -> bool {
    now.checked_sub(retention)
        .is_some_and(|horizon| at < horizon)
}

/// Each part of the path is the live child of that name, else its most recently tombstoned one, so a file under
/// a deleted folder is reachable (owner decision 2026-11-14). An invalid path is `NotFound`.
async fn resolve(tx: &mut DbTx, account: Uuid, path: &str) -> Result<Uuid, RestoreError> {
    let names = split_path(path).map_err(|_| RestoreError::NotFound)?;
    let mut node = reader_repo::root(tx, account)
        .await
        .map_err(unexpected)?
        .ok_or(RestoreError::NotFound)?;
    for name in &names {
        let key = name_key(name);
        node = reader_repo::child_by_key(tx, account, node, key.as_str())
            .await
            .map_err(unexpected)?
            .ok_or(RestoreError::NotFound)?
            .0;
    }
    Ok(node)
}

/// The conflict helpers are shared with `publish`; their failures are all the unexpected tier here.
fn entry_error(error: EntryError) -> RestoreError {
    match error {
        EntryError::Call(TreeCommitError::Unexpected(fault)) => RestoreError::Unexpected(fault),
        EntryError::Call(other) => RestoreError::Unexpected(Box::new(other)),
        EntryError::Failed(failure) => {
            RestoreError::Unexpected(Box::new(RestoreFault::EntryFailed(failure)))
        }
    }
}

fn broken(what: &'static str) -> RestoreError {
    RestoreError::Unexpected(Box::new(RestoreFault::BrokenRow(what)))
}

fn unexpected(error: DbError) -> RestoreError {
    RestoreError::Unexpected(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);
    const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

    #[test]
    fn an_instant_older_than_the_retention_is_refused_and_its_edge_is_kept() {
        // Arrange
        let retention = 24 * HOUR;
        let edge = NOW.checked_sub(retention).expect("an edge");
        let before = Timestamp::from_unix_micros(edge.as_unix_micros() - 1);

        // Act
        let refused = [before, edge, NOW].map(|at| beyond_retention(NOW, retention, at));

        // Assert
        assert_eq!(refused, [true, false, false]);
    }
}
