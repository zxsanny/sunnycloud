//! The fake server behind the fake API: folders, and files that are idempotent by `client_key` and batches by
//! `commit_id`, as the real server's `start_upload` and `commits:batch` are.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use sync_core::SyncError;
use sync_core_proto::{
    BatchEntry, BatchResult, CommitOutcome, EntryResult, FileHash, ListingItem, ListingPage,
    NodeKind, NodeView,
};
use uuid::Uuid;

pub const ROOT: Uuid = Uuid::from_u128(1);

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: Uuid,
    pub parent: Uuid,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct File {
    pub node: Uuid,
    pub revision: Uuid,
    pub parent: Uuid,
    pub name: String,
    pub hash: FileHash,
}

#[derive(Default)]
pub struct ServerState {
    next: u128,
    pub folders: Vec<Folder>,
    /// By `client_key`.
    pub files: HashMap<Uuid, File>,
    batches: HashMap<Uuid, BatchResult>,
    pub published: usize,
    pub nothing_to_send: usize,
    /// Entries of a batch the server refuses, by name, with the wire code of the refusal.
    pub refused: HashMap<String, String>,
    /// A missing folder is answered `ParentNotFound` instead of `ParentGone`.
    pub missing_is_not_found: bool,
}

#[derive(Default)]
pub struct Server(Mutex<ServerState>);

impl ServerState {
    fn fresh(&mut self) -> Uuid {
        self.next += 1;
        Uuid::from_u128(0x1000 + self.next)
    }

    /// The root and every existing folder can take files.
    pub fn has_folder(&self, id: Uuid) -> bool {
        id == ROOT || self.folders.iter().any(|folder| folder.id == id)
    }

    pub fn folder_named(&self, parent: Uuid, name: &str) -> Option<Uuid> {
        self.folders
            .iter()
            .find(|folder| folder.parent == parent && folder.name == name)
            .map(|folder| folder.id)
    }
}

impl Server {
    pub fn state(&self) -> MutexGuard<'_, ServerState> {
        self.0.lock().expect("server state")
    }

    pub fn file_count(&self) -> usize {
        self.state().files.len()
    }

    /// The user deletes a folder on the web: later uploads into it are refused.
    pub fn delete_folder(&self, id: Uuid) {
        self.state().folders.retain(|folder| folder.id != id);
    }

    /// The error of an upload into a folder that is not there.
    pub fn missing_parent_error(&self) -> SyncError {
        if self.state().missing_is_not_found {
            SyncError::ParentNotFound
        } else {
            SyncError::ParentGone
        }
    }

    pub fn create_folder(&self, parent: Uuid, name: &str) -> Result<NodeView, SyncError> {
        let mut state = self.state();
        if state.folder_named(parent, name).is_some() {
            return Err(SyncError::NameConflict);
        }
        let id = state.fresh();
        state.folders.push(Folder {
            id,
            parent,
            name: name.to_owned(),
        });
        Ok(NodeView {
            node_id: id,
            parent_id: Some(parent),
            kind: NodeKind::Folder,
            name: name.to_owned(),
            current_revision: None,
        })
    }

    pub fn children(&self, parent: Uuid) -> ListingPage {
        let items = self
            .state()
            .folders
            .iter()
            .filter(|folder| folder.parent == parent)
            .map(|folder| ListingItem {
                node_id: folder.id,
                parent_id: Some(parent),
                kind: NodeKind::Folder,
                name: folder.name.clone(),
                current_revision: None,
                content_hash: None,
            })
            .collect();
        ListingPage { items, next: None }
    }

    /// `start_upload` of one key: a known key answers `NothingToSend` and no bytes are taken.
    pub fn commit_file(&self, key: Uuid, entry: File) -> CommitOutcome {
        let mut state = self.state();
        if let Some(existing) = state.files.get(&key) {
            let revision_id = existing.revision;
            state.nothing_to_send += 1;
            return CommitOutcome::NothingToSend { revision_id };
        }
        let outcome = CommitOutcome::Published {
            node_id: entry.node,
            revision_id: entry.revision,
        };
        state.files.insert(key, entry);
        state.published += 1;
        outcome
    }

    pub fn new_file(&self, parent: Uuid, name: &str, hash: FileHash) -> File {
        let mut state = self.state();
        File {
            node: state.fresh(),
            revision: state.fresh(),
            parent,
            name: name.to_owned(),
            hash,
        }
    }

    /// A repeated `commit_id` answers its stored result; a known key keeps its file.
    pub fn commit_batch(&self, commit_id: Uuid, entries: &[BatchEntry]) -> BatchResult {
        if let Some(stored) = self.state().batches.get(&commit_id) {
            return stored.clone();
        }
        let results = entries
            .iter()
            .map(|entry| {
                if !self.state().has_folder(entry.parent) {
                    let code = if self.state().missing_is_not_found {
                        "parent_not_found"
                    } else {
                        "parent_deleted"
                    };
                    return EntryResult::EntryError {
                        code: code.to_owned(),
                    };
                }
                if let Some(code) = self.state().refused.get(&entry.name).cloned() {
                    return EntryResult::EntryError { code };
                }
                let file = self.new_file(entry.parent, &entry.name, entry.file_hash);
                let (node_id, revision_id) = (file.node, file.revision);
                let existing = self.state().files.get(&entry.staged_key).cloned();
                if existing.is_none() {
                    self.commit_file(entry.staged_key, file);
                }
                let kept =
                    existing.map_or((node_id, revision_id), |kept| (kept.node, kept.revision));
                EntryResult::Committed {
                    node_id: kept.0,
                    revision_id: kept.1,
                    conflict_copy: None,
                }
            })
            .collect();
        let result = BatchResult { entries: results };
        self.state().batches.insert(commit_id, result.clone());
        result
    }
}
