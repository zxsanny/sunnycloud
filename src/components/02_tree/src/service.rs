//! Folder create, rename, move and delete. Each call is one short transaction: `AccountGuard::ensure_active`
//! first, the node writes, `FeedStamp::stamp` last, COMMIT, then `FeedNotifier::notify`. A failure rolls back
//! and consumes no `seq`. Only a move takes the account advisory lock.

use std::sync::Arc;

use contracts::{
    AccountGuard, AccountGuardError, FeedNotifier, FeedStamp, FeedStampError, NodeKind, NodeState,
    NodeView, Seq, TouchedNodes, TreeServiceError,
};
use sc_clock::Clock;
use sc_db::{Db, DbError, DbTx, Scope};
use sc_ids::new_id;
use sc_names::{ValidName, name_key, validate_name};
use uuid::Uuid;

use crate::node_repo::{self, NewFolder, NodeChange, NodeRow};
use crate::retry::run_retrying;

/// The unique index that keeps names unique per folder by `name_key`; it is the arbiter of a name conflict.
const NAME_INDEX: &str = "nodes_parent_name_key";

/// The tree mutations of `TreeService`. The trait itself is implemented once the path and device queries exist.
pub struct PgTreeService {
    db: Db,
    guard: Arc<dyn AccountGuard>,
    stamp: Arc<dyn FeedStamp>,
    notifier: Arc<dyn FeedNotifier>,
    clock: Arc<dyn Clock>,
}

impl PgTreeService {
    pub fn new(
        db: Db,
        guard: Arc<dyn AccountGuard>,
        stamp: Arc<dyn FeedStamp>,
        notifier: Arc<dyn FeedNotifier>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        PgTreeService {
            db,
            guard,
            stamp,
            notifier,
            clock,
        }
    }

    pub async fn create_folder(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<NodeView, TreeServiceError> {
        let name = valid(name)?;
        let key = name_key(&name);
        let view = run_retrying(&self.db, Scope::Account(account), async |tx| {
            self.ensure_active(tx, account).await?;
            let parent = node_repo::lock_for_share(tx, account, parent_id)
                .await
                .map_err(unexpected)?;
            usable_parent(parent)?;
            let now = self.clock.now_utc();
            let id = new_id(self.clock.as_ref());
            let folder = NewFolder {
                id,
                account,
                parent_id,
                name: name.as_str(),
                name_key: key.as_str(),
                now,
            };
            node_repo::insert_folder(tx, &folder)
                .await
                .map_err(name_conflict)?;
            let seq = self.stamp(tx, account, id).await?;
            Ok(NodeView {
                id,
                parent_id: Some(parent_id),
                kind: NodeKind::Folder,
                name: name.as_str().to_owned(),
                state: NodeState::Live,
                current_revision_id: None,
                size: None,
                content_hash: None,
                media_type: None,
                client_mtime: None,
                created_at: now,
                updated_at: now,
                seq,
            })
        })
        .await?;
        self.notifier.notify(account, view.seq);
        Ok(view)
    }

    /// Rename, move, or both. A case-only rename keeps the node id; a move is refused when it would put the node
    /// under itself. With no move and no new spelling (no name, or the exact stored name) the node is returned
    /// as it is: no write, no stamp, no notify.
    pub async fn update_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_parent: Option<Uuid>,
    ) -> Result<NodeView, TreeServiceError> {
        let new_name = new_name.map(valid).transpose()?;
        let new_key = new_name.as_ref().map(name_key);
        let (view, stamped) = run_retrying(&self.db, Scope::Account(account), async |tx| {
            self.ensure_active(tx, account).await?;
            if new_parent.is_none() {
                let current = node_repo::view(tx, account, node_id)
                    .await
                    .map_err(unexpected)?
                    .filter(|view| view.state == NodeState::Live)
                    .ok_or(TreeServiceError::NotFound)?;
                if current.parent_id.is_none() {
                    return Err(TreeServiceError::IsRoot);
                }
                if new_name
                    .as_ref()
                    .is_none_or(|name| name.as_str() == current.name)
                {
                    return Ok((current, false));
                }
            }
            if new_parent.is_some() {
                tx.advisory_lock_account(account)
                    .await
                    .map_err(unexpected)?;
            }
            let node = node_repo::lock_for_update(tx, account, node_id)
                .await
                .map_err(unexpected)?
                .filter(|node| !node.deleted)
                .ok_or(TreeServiceError::NotFound)?;
            let Some(current_parent) = node.parent_id else {
                return Err(TreeServiceError::IsRoot);
            };
            let parent_id = match new_parent {
                Some(target) => {
                    check_move(tx, account, node_id, target).await?;
                    target
                }
                None => current_parent,
            };
            let change = NodeChange {
                id: node_id,
                account,
                parent_id,
                name: new_name.as_ref().map(ValidName::as_str),
                name_key: new_key.as_ref().map(|key| key.as_str()),
                now: self.clock.now_utc(),
            };
            node_repo::update(tx, &change)
                .await
                .map_err(name_conflict)?;
            let mut view = node_repo::view(tx, account, node_id)
                .await
                .map_err(unexpected)?
                .ok_or_else(|| unexpected(DbError::Other("updated node not found".into())))?;
            view.seq = self.stamp(tx, account, node_id).await?;
            Ok((view, true))
        })
        .await?;
        if stamped {
            self.notifier.notify(account, view.seq);
        }
        Ok(view)
    }

    /// Idempotent: a tombstoned node succeeds with no change and no stamp. A file whose current revision is not
    /// `base_revision` is refused with that revision. A folder gets one tombstone and `cascade_pending`; its
    /// descendants are marked later by the cascade.
    pub async fn delete_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        base_revision: Option<Uuid>,
    ) -> Result<(), TreeServiceError> {
        let stamped = run_retrying(&self.db, Scope::Account(account), async |tx| {
            self.ensure_active(tx, account).await?;
            let node = node_repo::lock_for_update(tx, account, node_id)
                .await
                .map_err(unexpected)?
                .ok_or(TreeServiceError::NotFound)?;
            if node.parent_id.is_none() {
                return Err(TreeServiceError::IsRoot);
            }
            if node.deleted {
                return Ok(None);
            }
            check_base(&node, base_revision)?;
            node_repo::tombstone(tx, account, node_id, self.clock.now_utc())
                .await
                .map_err(unexpected)?;
            self.stamp(tx, account, node_id).await.map(Some)
        })
        .await?;
        if let Some(seq) = stamped {
            self.notifier.notify(account, seq);
        }
        Ok(())
    }

    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), TreeServiceError> {
        self.guard
            .ensure_active(tx, account)
            .await
            .map_err(|error| match error {
                AccountGuardError::AccountNotActive => TreeServiceError::AccountNotActive,
                AccountGuardError::Unexpected(fault) => TreeServiceError::Unexpected(fault),
            })
    }

    async fn stamp(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        node: Uuid,
    ) -> Result<Seq, TreeServiceError> {
        self.stamp
            .stamp(tx, account, TouchedNodes(vec![node]))
            .await
            .map_err(|error| match error {
                FeedStampError::Unexpected(fault) => TreeServiceError::Unexpected(fault),
                missing @ FeedStampError::AccountFeedMissing => {
                    TreeServiceError::Unexpected(Box::new(missing))
                }
            })
    }
}

/// The target folder is locked like a create's parent, then the chain above it is walked for the moved node.
async fn check_move(
    tx: &mut DbTx,
    account: Uuid,
    node_id: Uuid,
    target: Uuid,
) -> Result<(), TreeServiceError> {
    if target == node_id {
        return Err(TreeServiceError::Cycle);
    }
    let parent = node_repo::lock_for_share(tx, account, target)
        .await
        .map_err(unexpected)?;
    usable_parent(parent)?;
    if node_repo::reaches(tx, account, target, node_id)
        .await
        .map_err(unexpected)?
    {
        return Err(TreeServiceError::Cycle);
    }
    Ok(())
}

fn check_base(node: &NodeRow, base_revision: Option<Uuid>) -> Result<(), TreeServiceError> {
    match (node.kind, base_revision, node.current_revision_id) {
        (NodeKind::File, Some(base), Some(current)) if base != current => {
            Err(TreeServiceError::EditedSince {
                current_revision: current,
            })
        }
        _ => Ok(()),
    }
}

fn usable_parent(parent: Option<NodeRow>) -> Result<(), TreeServiceError> {
    match parent {
        None => Err(TreeServiceError::NotFound),
        Some(parent) if parent.kind != NodeKind::Folder => Err(TreeServiceError::NotAFolder),
        Some(parent) if parent.deleted => Err(TreeServiceError::ParentDeleted),
        Some(_) => Ok(()),
    }
}

fn valid(name: &str) -> Result<ValidName, TreeServiceError> {
    validate_name(name).map_err(|reason| TreeServiceError::NameInvalid(reason.to_string()))
}

fn name_conflict(error: DbError) -> TreeServiceError {
    match error {
        DbError::UniqueViolation {
            constraint: Some(ref constraint),
        } if constraint == NAME_INDEX => TreeServiceError::NameConflict,
        other => unexpected(other),
    }
}

fn unexpected(error: DbError) -> TreeServiceError {
    TreeServiceError::Unexpected(Box::new(error))
}
