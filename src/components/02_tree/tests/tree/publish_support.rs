//! Entries, blob rows and the test effects of the `publish` tests (AZ-3709). The test effects stand for 03's and
//! 08's effects at the `CommitEffect` port: they record what they receive, mark their calls in the statement log,
//! and fail, replay, pause or write a row when told to.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    Begin, CommitActor, CommitEffect, EffectError, EntryOutcome, NewRevision, PublishEntry,
    PublishedEntry, StoredResponse, Target, async_trait,
};
use sc_db::DbTx;
use sc_ids::{FileHash, Hash32};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::mutation_support::Tree;
use sc_testdb::statements;

pub const SIZE: i64 = 100;

pub fn actor(account: Uuid) -> CommitActor {
    CommitActor {
        account_id: account,
        device_label: "laptop".to_owned(),
    }
}

/// A content hash of its own for every call with a different `seed`.
pub fn hash_of(seed: Uuid) -> FileHash {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(seed.as_bytes());
    FileHash::from_bytes(bytes)
}

pub fn revision(blob_id: Uuid) -> NewRevision {
    let revision_id = Uuid::new_v4();
    NewRevision {
        revision_id,
        blob_id,
        size: SIZE,
        content_hash: hash_of(revision_id),
        blocklist: vec![Hash32::from_bytes([3; 32])],
        media_type: Some("text/plain".to_owned()),
        client_mtime: None,
        client_meta: None,
        reserved_bytes: 0,
        import_hint: false,
    }
}

pub fn new_file(parent_id: Uuid, name: &str, blob_id: Uuid) -> PublishEntry {
    PublishEntry {
        target: Target::New {
            parent_id,
            name: name.to_owned(),
        },
        revision: revision(blob_id),
        vault: false,
    }
}

pub fn existing(node_id: Uuid, base_revision: Option<Uuid>, blob_id: Uuid) -> PublishEntry {
    PublishEntry {
        target: Target::Existing {
            node_id,
            base_revision,
        },
        revision: revision(blob_id),
        vault: false,
    }
}

impl Tree {
    /// `count` blob rows of `account` in `state` (size 1 when committed), inserted in one statement.
    pub async fn blobs_of(&self, account: Uuid, count: usize, state: &str) -> Vec<Uuid> {
        let ids: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
        let mut admin = self.fixture.db.admin().await;
        sqlx::query(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             SELECT id, $2, $3, CASE WHEN $3 = 'committed' THEN 1 END, '\\x00', 1, 'epoch'::timestamptz \
             FROM unnest($1::uuid[]) AS id",
        )
        .bind(&ids)
        .bind(account)
        .bind(state)
        .execute(&mut admin)
        .await
        .expect("insert blobs");
        crate::mutation_support::close(admin).await;
        ids
    }

    pub async fn blobs(&self, count: usize) -> Vec<Uuid> {
        self.blobs_of(self.account, count, "committed").await
    }

    /// `(used_bytes, reserved_bytes)` of the account.
    pub async fn quota(&self) -> (i64, i64) {
        let used = self
            .count(&format!(
                "SELECT used_bytes FROM quota WHERE account_id = '{}'",
                self.account
            ))
            .await;
        let reserved = self
            .count(&format!(
                "SELECT reserved_bytes FROM quota WHERE account_id = '{}'",
                self.account
            ))
            .await;
        (used, reserved)
    }

    pub async fn set_reserved(&self, bytes: i64) {
        self.exec(&format!(
            "UPDATE quota SET reserved_bytes = {bytes} WHERE account_id = '{}'",
            self.account
        ))
        .await;
    }

    pub async fn revisions(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM revisions WHERE account_id = '{}'",
            self.account
        ))
        .await
    }
}

/// What `on_begin` does.
pub enum OnBegin {
    Proceed,
    Replay(StoredResponse),
    /// Signals `reached`, then waits for `resume`.
    Pause {
        reached: Arc<Notify>,
        resume: Arc<Notify>,
    },
}

/// What `on_batch` does after recording the outcomes.
pub enum OnBatch {
    Proceed,
    Fail(EffectError),
    /// Inserts one row into the probe table `effect_rows`.
    WriteRow(Uuid),
}

/// One call the effect received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Begin,
    Entry(PublishedEntry),
    Batch(Vec<EntryOutcome>),
}

/// The recording test effect; `fail_entries` names revisions whose `on_entry` returns that error.
pub struct TestEffect {
    pub begin: OnBegin,
    pub batch: OnBatch,
    pub fail_entries: HashMap<Uuid, EffectError>,
    calls: Mutex<Vec<Call>>,
}

impl TestEffect {
    pub fn new(begin: OnBegin, batch: OnBatch) -> Arc<TestEffect> {
        Arc::new(TestEffect {
            begin,
            batch,
            fail_entries: HashMap::new(),
            calls: Mutex::default(),
        })
    }

    pub fn recording() -> Arc<TestEffect> {
        TestEffect::new(OnBegin::Proceed, OnBatch::Proceed)
    }

    pub fn failing_entries(fail_entries: HashMap<Uuid, EffectError>) -> Arc<TestEffect> {
        Arc::new(TestEffect {
            begin: OnBegin::Proceed,
            batch: OnBatch::Proceed,
            fail_entries,
            calls: Mutex::default(),
        })
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, call: Call) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
    }
}

#[async_trait]
impl CommitEffect for TestEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        statements::mark("effect:on_begin");
        self.record(Call::Begin);
        match &self.begin {
            OnBegin::Proceed => Ok(Begin::Proceed),
            OnBegin::Replay(response) => Ok(Begin::Replay(response.clone())),
            OnBegin::Pause { reached, resume } => {
                reached.notify_one();
                resume.notified().await;
                Ok(Begin::Proceed)
            }
        }
    }

    async fn on_entry(&self, _tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        statements::mark("effect:on_entry");
        self.record(Call::Entry(entry.clone()));
        match self.fail_entries.get(&entry.revision_id) {
            Some(error) => Err(*error),
            None => Ok(()),
        }
    }

    async fn on_batch(&self, tx: &mut DbTx, outcomes: &[EntryOutcome]) -> Result<(), EffectError> {
        statements::mark("effect:on_batch");
        self.record(Call::Batch(outcomes.to_vec()));
        match &self.batch {
            OnBatch::Proceed => Ok(()),
            OnBatch::Fail(error) => Err(*error),
            OnBatch::WriteRow(id) => {
                sqlx::query("INSERT INTO effect_rows (id) VALUES ($1)")
                    .bind(id)
                    .execute(tx.conn())
                    .await
                    .map_err(|_| EffectError::Batch(sc_error::ErrorCode::Internal))?;
                Ok(())
            }
        }
    }
}

pub fn effects(effect: &Arc<TestEffect>) -> Vec<Arc<dyn CommitEffect>> {
    vec![effect.clone() as Arc<dyn CommitEffect>]
}
