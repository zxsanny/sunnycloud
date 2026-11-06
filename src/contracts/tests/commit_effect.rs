//! AC-2 (effects supplied per call), AC-3 (closed outcome sets) and the `&mut DbTx` doubles of the commit
//! path (they compile; they run in the component tests that own a database).

mod common;

use std::sync::Arc;

use common::{Calls, NOW, id};
use contracts::{
    Begin, CommitActor, CommitEffect, EffectError, EntryFailure, EntryOutcome, EntryResult,
    NewRevision, PublishEntry, PublishOutcome, PublishResult, PublishedEntry, StoredResponse,
    Target, TreeCommit, TreeCommitError, async_trait,
};
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_ids::FileHash;
use uuid::Uuid;

/// Returns each of the three protocol results; never run, because a `DbTx` needs a live database.
struct TestEffect;

#[async_trait]
impl CommitEffect for TestEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Replay(StoredResponse(vec![1])))
    }

    async fn on_entry(&self, _tx: &mut DbTx, _entry: &PublishedEntry) -> Result<(), EffectError> {
        Err(EffectError::Entry(ErrorCode::NeedBlocks))
    }

    async fn on_batch(
        &self,
        _tx: &mut DbTx,
        _outcomes: &[EntryOutcome],
    ) -> Result<(), EffectError> {
        Err(EffectError::Batch(ErrorCode::EditedSince))
    }
}

/// Records how many effects each `publish` call received.
#[derive(Default)]
struct RecordingCommit {
    calls: Calls,
    effect_counts: std::sync::Mutex<Vec<usize>>,
}

#[async_trait]
impl TreeCommit for RecordingCommit {
    async fn publish(
        &self,
        _actor: CommitActor,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
        _now: sc_clock::Timestamp,
    ) -> Result<PublishResult, TreeCommitError> {
        self.calls.record("publish");
        self.effect_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(effects.len());
        if entries.is_empty() {
            return Ok(PublishResult::Replayed(StoredResponse(vec![1])));
        }
        Ok(PublishResult::Published(
            entries
                .iter()
                .map(|_| EntryResult::Failed(EntryFailure::EffectFailed(ErrorCode::NeedBlocks)))
                .collect(),
        ))
    }
}

fn actor() -> CommitActor {
    CommitActor {
        account_id: id(),
        device_label: "laptop".to_owned(),
    }
}

fn entry() -> PublishEntry {
    PublishEntry {
        target: Target::New {
            parent_id: id(),
            name: "a".to_owned(),
        },
        revision: NewRevision {
            revision_id: Uuid::from_u128(2),
            blob_id: Uuid::from_u128(3),
            size: 1,
            content_hash: FileHash::from_bytes([0; 32]),
            blocklist: Vec::new(),
            media_type: None,
            client_mtime: None,
            client_meta: None,
            reserved_bytes: 1,
            import_hint: false,
        },
        vault: false,
    }
}

#[tokio::test]
async fn a_test_effect_is_passed_to_publish_as_arc_dyn_and_the_double_records_the_list() {
    let tree = Arc::new(RecordingCommit::default());
    let port: Arc<dyn TreeCommit> = tree.clone();
    let effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(TestEffect), Arc::new(TestEffect)];

    let result = port
        .publish(actor(), vec![entry()], effects, NOW)
        .await
        .expect("publish");

    assert_eq!(tree.calls.sorted(), vec!["publish"]);
    assert_eq!(*tree.effect_counts.lock().expect("lock"), vec![2]);
    assert_eq!(
        result,
        PublishResult::Published(vec![EntryResult::Failed(EntryFailure::EffectFailed(
            ErrorCode::NeedBlocks
        ))])
    );
}

#[tokio::test]
async fn publish_without_entries_replays_a_stored_response() {
    let tree = RecordingCommit::default();

    let result = tree
        .publish(actor(), Vec::new(), Vec::new(), NOW)
        .await
        .expect("publish");

    let PublishResult::Replayed(stored) = result else {
        panic!("expected a replay");
    };
    assert_eq!(stored, StoredResponse(vec![1]));
}

fn outcome(value: &PublishOutcome) -> &'static str {
    match value {
        PublishOutcome::Created => "Created",
        PublishOutcome::Updated => "Updated",
        PublishOutcome::Replaced => "Replaced",
        PublishOutcome::ConflictCopied { .. } => "ConflictCopied",
        PublishOutcome::CreatedAsCopy { .. } => "CreatedAsCopy",
        PublishOutcome::Revived => "Revived",
        PublishOutcome::NoChange { .. } => "NoChange",
    }
}

fn effect_error(value: &EffectError) -> &'static str {
    match value {
        EffectError::Entry(_) => "Entry",
        EffectError::Batch(_) => "Batch",
    }
}

#[test]
fn publish_outcome_has_exactly_the_seven_contract_variants() {
    let all = [
        PublishOutcome::Created,
        PublishOutcome::Updated,
        PublishOutcome::Replaced,
        PublishOutcome::ConflictCopied { copy_node: id() },
        PublishOutcome::CreatedAsCopy { node: id() },
        PublishOutcome::Revived,
        PublishOutcome::NoChange {
            existing_revision: id(),
        },
    ];

    let names: Vec<_> = all.iter().map(outcome).collect();

    assert_eq!(
        names,
        [
            "Created",
            "Updated",
            "Replaced",
            "ConflictCopied",
            "CreatedAsCopy",
            "Revived",
            "NoChange"
        ]
    );
}

#[test]
fn effect_error_has_exactly_entry_and_batch() {
    let all = [
        EffectError::Entry(ErrorCode::NotFound),
        EffectError::Batch(ErrorCode::NotFound),
    ];

    let names: Vec<_> = all.iter().map(effect_error).collect();

    assert_eq!(names, ["Entry", "Batch"]);
}

#[test]
fn effect_error_names_its_code_in_the_message() {
    let message = EffectError::Batch(ErrorCode::EditedSince).to_string();

    assert!(message.contains("edited_since"), "{message}");
}

#[test]
fn begin_distinguishes_proceed_from_replay() {
    let proceed = Begin::Proceed;
    let replay = Begin::Replay(StoredResponse(vec![9]));

    assert_ne!(proceed, replay);
}
