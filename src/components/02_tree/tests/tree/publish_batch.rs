//! AZ-3709 AC-7 (IT-02-9: one savepoint per entry in a 500-entry batch) and AC-8 (IT-02-12: no committed revision
//! names an uncommitted blob, with the real `BlobRegistry::mark_committed_in_tx` in the test effect).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Begin, BlobRegistry as _, CommitEffect, EffectError, EntryFailure, EntryOutcome, EntryResult,
    PublishOutcome, PublishResult, PublishedEntry, TreeCommit, async_trait,
};
use sc_config::Secret;
use sc_db::DbTx;
use sc_error::ErrorCode;
use sunnycloud_blobstore::{BlobRegistry, BlobstoreConfig};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{SIZE, TestEffect, actor, effects, new_file};

const BATCH: usize = 500;
const RESERVED: i64 = 10;
const RESERVED_400: i64 = 7;

#[tokio::test]
async fn three_bad_entries_of_five_hundred_fail_alone_and_the_rest_commit_under_one_seq() {
    // Arrange
    let tree = Tree::new().await;
    let gone = tree.folder(tree.root, "Gone").await;
    tree.tombstone(gone).await;
    let blobs = tree.blobs(BATCH).await;
    let entries: Vec<_> = blobs
        .iter()
        .enumerate()
        .map(|(index, blob)| {
            let mut entry = match index {
                10 => new_file(gone, "f10.txt", *blob),
                250 => new_file(tree.root, "bad/name", *blob),
                _ => new_file(tree.root, &format!("f{index}.txt"), *blob),
            };
            entry.revision.reserved_bytes = if index == 400 { RESERVED_400 } else { RESERVED };
            entry
        })
        .collect();
    tree.set_reserved(RESERVED * (BATCH as i64 - 1) + RESERVED_400)
        .await;
    let effect = TestEffect::failing_entries(HashMap::from([(
        entries[400].revision.revision_id,
        EffectError::Entry(ErrorCode::UploadSessionGone),
    )]));

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            entries,
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("the batch commits");

    // Assert
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    let created = results
        .iter()
        .filter(|result| {
            matches!(
                result,
                EntryResult::Published {
                    outcome: PublishOutcome::Created,
                    ..
                }
            )
        })
        .count();
    assert_eq!(created, 497);
    assert_eq!(
        results[10],
        EntryResult::Failed(EntryFailure::ParentDeleted)
    );
    assert!(matches!(
        results[250],
        EntryResult::Failed(EntryFailure::NameInvalid(_))
    ));
    assert_eq!(
        results[400],
        EntryResult::Failed(EntryFailure::EffectFailed(ErrorCode::UploadSessionGone))
    );
    assert_eq!(tree.seq().await, 1, "the counter advanced by exactly 1");
    let stamped = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{}' AND kind = 'file' AND seq = 1",
            tree.account
        ))
        .await;
    assert_eq!(stamped, 497, "all published nodes carry the one seq");
    assert_eq!(tree.revisions().await, 497);
    assert_eq!(
        tree.quota().await,
        (497 * SIZE, 2 * RESERVED + RESERVED_400),
        "entry 400's release rolled back with its savepoint; the others released"
    );
    tree.finish().await;
}

/// The test double of 03's blob effect: flips the entry's blob to committed with the real registry, then fails
/// when its fate says so.
struct BlobEffect {
    registry: Arc<BlobRegistry>,
    account: Uuid,
    blobs: HashMap<Uuid, Uuid>,
    fates: HashMap<Uuid, EffectError>,
}

#[async_trait]
impl CommitEffect for BlobEffect {
    async fn on_begin(&self, _tx: &mut DbTx) -> Result<Begin, EffectError> {
        Ok(Begin::Proceed)
    }

    async fn on_entry(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError> {
        let blob = self.blobs[&entry.revision_id];
        self.registry
            .mark_committed_in_tx(tx, self.account, blob, entry.size)
            .await
            .map_err(|_| EffectError::Batch(ErrorCode::Internal))?;
        match self.fates.get(&entry.revision_id) {
            Some(error) => Err(*error),
            None => Ok(()),
        }
    }

    async fn on_batch(
        &self,
        _tx: &mut DbTx,
        _outcomes: &[EntryOutcome],
    ) -> Result<(), EffectError> {
        Ok(())
    }
}

fn registry() -> Arc<BlobRegistry> {
    let config = BlobstoreConfig {
        blob_root: Some("/srv/sunnycloud/blobs-unused-by-row-writes".to_owned()),
        retention: Duration::from_secs(7 * 86_400),
        orphan_sweep_age: Duration::from_secs(86_400),
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        blob_free_reserve_percent: 100,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: 1,
        blob_io_threads: None,
        master_key_file: Secret::new("unused".to_owned()),
        master_key_ring: None,
        master_key_current: None,
    };
    Arc::new(BlobRegistry::new(&config, sc_testdb::clock_at_start()).expect("registry"))
}

/// xorshift64: a fixed seed gives the same fates on every run.
struct Fates(u64);

impl Fates {
    fn next(&mut self) -> Option<EffectError> {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        match self.0 % 20 {
            0..=2 => Some(EffectError::Batch(ErrorCode::Internal)),
            3..=5 => Some(EffectError::Entry(ErrorCode::UploadSessionGone)),
            _ => None,
        }
    }
}

const DANGLING: &str = "SELECT count(*) FROM revisions r LEFT JOIN blobs b ON b.id = r.blob_id \
    WHERE b.id IS NULL OR b.state <> 'committed'";

const COMMITTED_WITHOUT_REVISION: &str = "SELECT count(*) FROM blobs b WHERE b.state = 'committed' \
    AND NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = b.id)";

#[tokio::test]
async fn no_committed_revision_names_an_uncommitted_blob_under_random_effect_failures() {
    // Arrange
    const PUBLISHES: usize = 300;
    let tree = Tree::new().await;
    let registry = registry();
    let blobs = tree.blobs_of(tree.account, PUBLISHES, "uploading").await;
    let mut fates = Fates(0x5eed_2026_1112);
    let (mut entry_failures, mut batch_failures, mut committed) = (0, 0, 0);
    let mut probes = Vec::with_capacity(PUBLISHES);

    // Act
    for (index, blob) in blobs.iter().enumerate() {
        let entry = new_file(tree.root, &format!("f{index}.txt"), *blob);
        let fate = fates.next();
        let effect = Arc::new(BlobEffect {
            registry: registry.clone(),
            account: tree.account,
            blobs: HashMap::from([(entry.revision.revision_id, *blob)]),
            fates: fate
                .into_iter()
                .map(|error| (entry.revision.revision_id, error))
                .collect(),
        });
        let result = tree
            .service
            .publish(
                actor(tree.account),
                vec![entry],
                vec![effect as Arc<dyn CommitEffect>],
                sc_testdb::START,
            )
            .await;
        match (fate, result) {
            (Some(EffectError::Batch(_)), Err(_)) => batch_failures += 1,
            (Some(EffectError::Entry(_)), Ok(PublishResult::Published(results)))
                if matches!(
                    results[0],
                    EntryResult::Failed(EntryFailure::EffectFailed(_))
                ) =>
            {
                entry_failures += 1;
            }
            (None, Ok(PublishResult::Published(results)))
                if matches!(results[0], EntryResult::Published { .. }) =>
            {
                committed += 1;
            }
            (fate, other) => panic!("publish {index} with fate {fate:?} gave {other:?}"),
        }
        probes.push((
            index,
            tree.count(DANGLING).await,
            tree.count(COMMITTED_WITHOUT_REVISION).await,
        ));
    }

    // Assert
    let violations: Vec<_> = probes
        .into_iter()
        .filter(|&(_, dangling, orphaned)| dangling != 0 || orphaned != 0)
        .collect();
    assert_eq!(
        violations,
        [],
        "(publish, dangling revisions, committed blobs without revision)"
    );
    assert!(entry_failures > 0 && batch_failures > 0 && committed > 0);
    assert_eq!(tree.revisions().await, committed);
    tree.finish().await;
}

#[tokio::test]
async fn a_blob_id_with_no_blobs_row_is_a_typed_entry_failure_with_no_revision() {
    // Arrange
    let tree = Tree::new().await;
    let entry = new_file(tree.root, "a.txt", Uuid::new_v4());
    let nodes = tree.nodes().await;

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry],
            effects(&TestEffect::recording()),
            sc_testdb::START,
        )
        .await
        .expect("the call itself succeeds");

    // Assert
    assert_eq!(
        result,
        PublishResult::Published(vec![EntryResult::Failed(EntryFailure::BlocklistMismatch)])
    );
    assert_eq!(tree.revisions().await, 0);
    assert_eq!(tree.nodes().await, nodes);
    assert_eq!(tree.seq().await, 0);
    tree.finish().await;
}
