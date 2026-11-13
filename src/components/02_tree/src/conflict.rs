//! The conflict outcomes of `publish` (ADR-005, ADR-007): a stale base keeps the superseded revision in a conflict
//! copy beside the file, a taken name gives a new file a conflict-copy name, and a commit on a deleted file revives
//! it with its tombstoned ancestors. Every name comes from helper 06's `conflict_copy_name` (UTC from the injected
//! clock, the committing device's label); every name write runs inside the entry's savepoint, so the unique name
//! index never aborts the transaction, and a race lost on it moves on to the next free name.

use std::collections::HashSet;

use contracts::{PublishOutcome, TreeCommitError};
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::new_id;
use sc_names::{NameKey, ValidName, conflict_copy_name, name_key, validate_name};
use thiserror::Error;
use uuid::Uuid;

use crate::conflict_repo::{self, Tombstoned};
use crate::publish::{Committing, EntryError, Written, insert_revision};
use crate::publish_repo::{self, FileRow, NewFile};
use crate::service::PgTreeService;

/// The unique index that keeps names unique per folder; on commit a hit on it means "take the next name".
const NAME_INDEX: &str = "nodes_parent_name_key";

/// Races lost on the name index (a concurrent rename or folder create took the chosen name) before an entry gives
/// up with an unexpected failure.
const MAX_LOST_RACES: u32 = 8;

#[derive(Debug, Error)]
#[error("no free conflict-copy name after {0} races lost on the name index")]
struct NameRacesLost(u32);

#[derive(Debug, Error)]
#[error("tree row breaks an invariant: {0}")]
struct BrokenRow(&'static str);

/// The inputs of every conflict-copy name of one call: the time, read once from the injected clock, and the label
/// of the committing device.
pub(crate) struct Naming<'a> {
    pub at: Timestamp,
    pub device_label: &'a str,
}

/// The conflict-copy names of one contested name in one folder. The keys found taken are kept, so the helper
/// moves on to the next ` (n)` suffix.
struct CopyNamer<'a> {
    contested: ValidName,
    naming: &'a Naming<'a>,
    parent: Uuid,
    taken: HashSet<NameKey>,
    races: u32,
}

impl<'a> CopyNamer<'a> {
    fn new(contested: &str, naming: &'a Naming<'a>, parent: Uuid) -> Result<Self, EntryError> {
        let contested = validate_name(contested).map_err(|_| broken("a stored name is invalid"))?;
        Ok(CopyNamer {
            contested,
            naming,
            parent,
            taken: HashSet::new(),
            races: 0,
        })
    }

    /// The first conflict-copy name no live node of the folder holds, with its key.
    async fn next(
        &mut self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(ValidName, NameKey), DbError> {
        loop {
            let (name, key) = self.candidate();
            if !conflict_repo::name_taken(tx, account, self.parent, key.as_str()).await? {
                return Ok((name, key));
            }
            self.taken.insert(key);
        }
    }

    /// The helper's first name outside the keys known to be taken.
    fn candidate(&self) -> (ValidName, NameKey) {
        let name = conflict_copy_name(
            &self.contested,
            self.naming.at,
            self.naming.device_label,
            &self.taken,
        );
        let key = name_key(&name);
        (name, key)
    }

    /// `key` was taken between the lookup and the write; the next call picks the following name.
    fn lost(&mut self, key: NameKey) -> Result<(), EntryError> {
        self.races += 1;
        if self.races > MAX_LOST_RACES {
            return Err(EntryError::Call(TreeCommitError::Unexpected(Box::new(
                NameRacesLost(MAX_LOST_RACES),
            ))));
        }
        self.taken.insert(key);
        Ok(())
    }
}

/// Inserts `file` under a conflict-copy name of `file.name` (the contested name; `file.name_key` is not read) in
/// its folder: `CreatedAsCopy`, and the copy node of `ConflictCopied`.
pub(crate) async fn insert_as_copy(
    tx: &mut DbTx,
    file: &NewFile<'_>,
    naming: &Naming<'_>,
) -> Result<(), EntryError> {
    let mut namer = CopyNamer::new(file.name, naming, file.parent_id)?;
    loop {
        let (name, key) = namer.next(tx, file.account).await?;
        let copy = NewFile {
            name: name.as_str(),
            name_key: key.as_str(),
            ..*file
        };
        if publish_repo::insert_file(tx, &copy).await? {
            return Ok(());
        }
        namer.lost(key)?;
    }
}

impl PgTreeService {
    /// `ConflictCopied`: the new revision becomes current, and the revision it supersedes moves (the same row and
    /// blob, no byte copy) to a new sibling under a conflict-copy name. Both nodes are touched.
    pub(crate) async fn keep_superseded(
        &self,
        tx: &mut DbTx,
        at: &Committing<'_>,
        node_id: Uuid,
        file: &FileRow,
        superseded: Uuid,
    ) -> Result<Written, EntryError> {
        let (account, entry, now) = (at.account, at.entry, at.now);
        let parent_id = file
            .parent_id
            .ok_or_else(|| broken("a file without a parent"))?;
        let copy_node = new_id(self.clock.as_ref());
        let template = NewFile {
            id: copy_node,
            account,
            parent_id,
            name: file.name.as_str(),
            name_key: "",
            revision_id: superseded,
            now,
        };
        insert_as_copy(tx, &template, at.naming).await?;
        conflict_repo::reparent_revision(tx, account, superseded, copy_node).await?;
        insert_revision(tx, account, node_id, entry, now).await?;
        publish_repo::set_current(tx, account, node_id, entry.revision.revision_id, now).await?;
        Ok(Written {
            outcome: PublishOutcome::ConflictCopied { copy_node },
            node_id,
            revision_id: entry.revision.revision_id,
            counted: None,
            revived: Vec::new(),
        })
    }

    /// Edit beats delete: the tombstoned ancestors (farthest first) and the file itself when tombstoned are
    /// revived, each with `cascade_pending` cleared, and the new revision becomes current whatever the base, with
    /// no conflict copy. A name taken meanwhile gives that node a conflict-copy name; when it is the file's own
    /// name the outcome is `CreatedAsCopy` (03 then tells the client to read the name again), otherwise `Revived`.
    pub(crate) async fn revive(
        &self,
        tx: &mut DbTx,
        at: &Committing<'_>,
        node_id: Uuid,
        file: &FileRow,
        ancestors: Vec<Tombstoned>,
    ) -> Result<Written, EntryError> {
        let (account, entry, now) = (at.account, at.entry, at.now);
        for ancestor in &ancestors {
            revive_node(tx, account, ancestor, at.naming, now).await?;
        }
        let renamed = if file.deleted {
            let parent_id = file
                .parent_id
                .ok_or_else(|| broken("a file without a parent"))?;
            let own = Tombstoned {
                id: node_id,
                parent_id,
                name: file.name.clone(),
            };
            revive_node(tx, account, &own, at.naming, now).await?
        } else {
            false
        };
        insert_revision(tx, account, node_id, entry, now).await?;
        publish_repo::set_current(tx, account, node_id, entry.revision.revision_id, now).await?;
        Ok(Written {
            outcome: if renamed {
                PublishOutcome::CreatedAsCopy { node: node_id }
            } else {
                PublishOutcome::Revived
            },
            node_id,
            revision_id: entry.revision.revision_id,
            counted: file.counted().copied(),
            revived: ancestors.iter().map(|ancestor| ancestor.id).collect(),
        })
    }
}

/// Revives one node under its own name, or under a conflict-copy name when its name is taken; `true` when renamed.
async fn revive_node(
    tx: &mut DbTx,
    account: Uuid,
    node: &Tombstoned,
    naming: &Naming<'_>,
    now: Timestamp,
) -> Result<bool, EntryError> {
    if try_revive(tx, account, node.id, None, now).await? {
        return Ok(false);
    }
    let mut namer = CopyNamer::new(&node.name, naming, node.parent_id)?;
    loop {
        let (name, key) = namer.next(tx, account).await?;
        let rename = Some((name.as_str(), key.as_str()));
        if try_revive(tx, account, node.id, rename, now).await? {
            return Ok(true);
        }
        namer.lost(key)?;
    }
}

/// An `UPDATE` cannot skip a taken name the way the insert does, so it runs in a nested savepoint: a hit on the
/// name index rolls back only that statement and answers `false`.
async fn try_revive(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    rename: Option<(&str, &str)>,
    now: Timestamp,
) -> Result<bool, DbError> {
    match tx
        .savepoint(async |tx| conflict_repo::revive(tx, account, id, rename, now).await)
        .await
    {
        Ok(()) => Ok(true),
        Err(DbError::UniqueViolation {
            constraint: Some(ref constraint),
        }) if constraint == NAME_INDEX => Ok(false),
        Err(other) => Err(other),
    }
}

fn broken(what: &'static str) -> EntryError {
    EntryError::Call(TreeCommitError::Unexpected(Box::new(BrokenRow(what))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-04 09:07 UTC.
    const AT: Timestamp = Timestamp::from_unix_micros(1_793_783_220_000_000);

    fn naming() -> Naming<'static> {
        Naming {
            at: AT,
            device_label: "phone-2",
        }
    }

    #[test]
    fn a_name_the_lookup_reports_taken_moves_the_helper_to_its_numeric_suffix() {
        // Arrange
        let naming = naming();
        let mut namer = CopyNamer::new("report.docx", &naming, Uuid::new_v4())
            .unwrap_or_else(|_| panic!("a valid contested name"));
        let (first, first_key) = namer.candidate();

        // Act
        namer.taken.insert(first_key);
        let (second, _) = namer.candidate();

        // Assert
        assert_eq!(
            first.as_str(),
            "report (conflict copy 2026-11-04 09-07 phone-2).docx"
        );
        assert_eq!(
            second.as_str(),
            "report (conflict copy 2026-11-04 09-07 phone-2) (2).docx"
        );
    }

    #[test]
    fn races_lost_on_the_name_index_are_bounded() {
        // Arrange
        let naming = naming();
        let mut namer = CopyNamer::new("a.txt", &naming, Uuid::new_v4())
            .unwrap_or_else(|_| panic!("a valid contested name"));

        // Act
        let within: Vec<bool> = (0..MAX_LOST_RACES)
            .map(|_| namer.lost(namer.candidate().1).is_ok())
            .collect();
        let beyond = namer.lost(namer.candidate().1);

        // Assert
        assert!(within.iter().all(|ok| *ok), "{within:?}");
        assert!(matches!(
            beyond,
            Err(EntryError::Call(TreeCommitError::Unexpected(_)))
        ));
    }
}
