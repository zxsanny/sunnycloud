//! A stateful account behind the scripted fixture: staged rows, the shared staged cap, batch receipts.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sc_error::ErrorCode;
use serde_json::json;
use sync_core_proto::{BatchCommit, BatchResult, EntryResult, hash_block};
use tokio::sync::Notify;
use uuid::Uuid;

use super::{Event, Timeline};
use crate::connect_support::valid_info;
use crate::support::{Answer, LoggedRequest, Route, Script};

#[derive(Clone, Copy)]
pub struct FakeConfig {
    pub batch_max_entries: usize,
    /// Advertised in `server-info` and enforced: staged rows of every device together never pass it.
    pub max_staged_per_account: usize,
    /// Rows other devices hold and never commit.
    pub other_staged: usize,
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            batch_max_entries: 500,
            max_staged_per_account: 1000,
            other_staged: 0,
        }
    }
}

#[derive(Default)]
struct State {
    staged: HashSet<Uuid>,
    /// Staged key to name, for every file a commit published.
    files: HashMap<Uuid, String>,
    publications: HashMap<Uuid, u32>,
    receipts: HashMap<Uuid, (BatchCommit, BatchResult)>,
    applied: u32,
    replayed: u32,
    stage_requests: usize,
    cap_refusals: usize,
    bad_hash: u32,
    max_ours: usize,
    max_batch: usize,
    kill_after_stagings: Option<(usize, Arc<Notify>)>,
    kill_after_commit: Option<(Arc<Notify>, Duration)>,
}

pub struct Fake {
    config: FakeConfig,
    timeline: Timeline,
    state: Mutex<State>,
}

impl Fake {
    pub fn new(config: FakeConfig, timeline: &Timeline) -> Arc<Self> {
        Arc::new(Self {
            config,
            timeline: timeline.clone(),
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The routes this fake answers: `server-info`, the staged `PUT` and the batch commit.
    pub fn script(self: &Arc<Self>) -> Script {
        let mut info = valid_info();
        info["batch_max_entries"] = json!(self.config.batch_max_entries);
        info["max_staged_per_account"] = json!(self.config.max_staged_per_account);
        let stage = Arc::clone(self);
        let commit = Arc::clone(self);
        Script::new()
            .always(Route::ServerInfo, Answer::json(200, &info))
            .respond(Route::StagedUpload, move |request| stage.stage(request))
            .respond(Route::BatchCommit, move |request| commit.commit(request))
    }

    /// Notifies once the fake holds `count` new staged rows.
    pub fn notify_after_stagings(&self, count: usize) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.state().kill_after_stagings = Some((count, Arc::clone(&notify)));
        notify
    }

    /// Notifies when a first-time batch commit was applied and delays that answer by `delay`.
    pub fn notify_after_commit(&self, delay: Duration) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.state().kill_after_commit = Some((Arc::clone(&notify), delay));
        notify
    }

    fn stage(&self, request: &LoggedRequest) -> Answer {
        let key = request
            .target
            .rsplit('/')
            .next()
            .and_then(|segment| segment.parse::<Uuid>().ok())
            .expect("the staged route carries a UUID");
        self.timeline.push(Event::Stage(key));
        let mut state = self.state();
        state.stage_requests += 1;
        let sent = request.header("x-blake3").unwrap_or_default();
        let actual = hash_block(&request.body).expect("a small body is one block");
        if sent != actual.to_string() {
            state.bad_hash += 1;
            return Answer::problem_with("hash_mismatch", 422, "Hash mismatch");
        }
        if !state.staged.contains(&key) {
            if self.config.other_staged + state.staged.len() >= self.config.max_staged_per_account {
                state.cap_refusals += 1;
                return Answer::problem(ErrorCode::StagedCapReached);
            }
            state.staged.insert(key);
            state.max_ours = state.max_ours.max(state.staged.len());
            if let Some((count, notify)) = &state.kill_after_stagings
                && state.staged.len() == *count
            {
                notify.notify_one();
            }
        }
        Answer::json(
            200,
            &json!({ "staged_key": key, "size": request.body.len() }),
        )
    }

    fn commit(&self, request: &LoggedRequest) -> Answer {
        let commit: BatchCommit =
            serde_json::from_slice(&request.body).expect("the client sent a BatchCommit");
        self.timeline.push(Event::Commit(commit.commit_id));
        let mut state = self.state();
        if commit.entries.len() > self.config.batch_max_entries {
            return Answer::problem(ErrorCode::BatchTooLarge);
        }
        state.max_batch = state.max_batch.max(commit.entries.len());
        if let Some((stored_request, stored)) = state.receipts.get(&commit.commit_id) {
            if *stored_request != commit {
                return Answer::problem(ErrorCode::CommitIdReused);
            }
            let stored = stored.clone();
            state.replayed += 1;
            return Answer::json(200, &stored);
        }
        let mut results = Vec::with_capacity(commit.entries.len());
        for entry in &commit.entries {
            if state.staged.remove(&entry.staged_key) {
                state.files.insert(entry.staged_key, entry.name.clone());
                *state.publications.entry(entry.staged_key).or_default() += 1;
                results.push(EntryResult::Committed {
                    node_id: entry.staged_key,
                    revision_id: entry.staged_key,
                    conflict_copy: None,
                });
            } else {
                results.push(EntryResult::EntryError {
                    code: "upload_not_found".to_owned(),
                });
            }
        }
        let result = BatchResult { entries: results };
        state.applied += 1;
        state
            .receipts
            .insert(commit.commit_id, (commit, result.clone()));
        let answer = Answer::json(200, &result);
        match &state.kill_after_commit {
            Some((notify, delay)) => {
                notify.notify_one();
                answer.delay(*delay)
            }
            None => answer,
        }
    }

    pub fn file_count(&self) -> usize {
        self.state().files.len()
    }

    /// Keys published more than once.
    pub fn duplicates(&self) -> Vec<Uuid> {
        self.state()
            .publications
            .iter()
            .filter(|(_, times)| **times > 1)
            .map(|(key, _)| *key)
            .collect()
    }

    pub fn staged_count(&self) -> usize {
        self.state().staged.len()
    }

    /// Batches applied for the first time.
    pub fn applied(&self) -> u32 {
        self.state().applied
    }

    /// Commits answered from a stored receipt.
    pub fn replayed(&self) -> u32 {
        self.state().replayed
    }

    pub fn stage_requests(&self) -> usize {
        self.state().stage_requests
    }

    pub fn cap_refusals(&self) -> usize {
        self.state().cap_refusals
    }

    pub fn bad_hash(&self) -> u32 {
        self.state().bad_hash
    }

    /// Most rows this client had staged and uncommitted at once (other devices' rows not counted).
    pub fn max_staged_uncommitted(&self) -> usize {
        self.state().max_ours
    }

    /// Most entries one commit request carried.
    pub fn max_batch_entries(&self) -> usize {
        self.state().max_batch
    }
}
