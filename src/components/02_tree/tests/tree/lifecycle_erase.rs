//! AZ-3717 AC-2 (rows and GC rows move together, no advisory lock) and AC-3 (honest, non-increasing `remaining`,
//! root last, a restart between batches, 0 only after the sweeper) — IT-02-23 with the real 04 queue and sweeper.

use contracts::AccountLifecycleHook;
use uuid::Uuid;

use crate::lifecycle_support::{Erase, NODES, REVISIONS};
use crate::statements::StatementLog;
use crate::support::START;

const LIMIT: u32 = 50;
/// More batches than 1,000 nodes and 600 revisions need at 50 rows each; a run past it is a stuck erase.
const MAX_BATCHES: usize = 200;

/// The state after one batch, read as the superuser.
struct Probe {
    nodes: i64,
    revisions: i64,
    queued: i64,
    root: bool,
    /// Revisions still present whose blob has a `blob_gc` row.
    queued_survivors: i64,
    /// Blobs whose revision is gone but which are not queued `account_deleted`, due now.
    unqueued_orphans: i64,
}

async fn probe(erase: &Erase) -> Probe {
    let account = erase.tree.account;
    let tree = &erase.tree;
    Probe {
        nodes: erase.of_account("nodes").await,
        revisions: erase.of_account("revisions").await,
        queued: erase.of_account("blob_gc").await,
        root: tree
            .count(&format!(
                "SELECT count(*) FROM nodes WHERE id = '{}'",
                tree.root
            ))
            .await
            == 1,
        queued_survivors: tree
            .count(&format!(
                "SELECT count(*) FROM revisions r JOIN blob_gc g ON g.blob_id = r.blob_id \
                 WHERE r.account_id = '{account}'"
            ))
            .await,
        unqueued_orphans: tree
            .count(&format!(
                "SELECT count(*) FROM blobs b WHERE b.account_id = '{account}' \
                 AND NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = b.id) \
                 AND NOT (b.state = 'gc_pending' AND EXISTS (SELECT 1 FROM blob_gc g WHERE g.blob_id = b.id \
                 AND g.reason = 'account_deleted' \
                 AND g.due_at = 'epoch'::timestamptz + {} * interval '1 microsecond'))",
                START.as_unix_micros()
            ))
            .await,
    }
}

#[tokio::test]
async fn every_batch_queues_the_blobs_of_the_revisions_it_removes_without_an_advisory_lock() {
    // Arrange
    let erase = Erase::new().await;
    let seeded = erase.seed().await;
    erase.mark_deleting().await;
    let log = StatementLog::install();

    // Act
    let mut probes = Vec::new();
    for _ in 0..MAX_BATCHES {
        erase.erase(LIMIT).await;
        let after = probe(&erase).await;
        let done = after.nodes + after.revisions == 0;
        probes.push(after);
        if done {
            break;
        }
    }
    let statements = log.take();

    // Assert
    assert!(probes.iter().all(|p| p.queued_survivors == 0));
    assert!(probes.iter().all(|p| p.unqueued_orphans == 0));
    assert!(
        statements
            .iter()
            .all(|sql| !sql.to_lowercase().contains("advisory"))
    );
    let last = probes.last().expect("at least one batch");
    assert_eq!((last.nodes, last.revisions), (0, 0));
    assert_eq!(last.queued, REVISIONS);
    assert_eq!(seeded.blobs.len() as i64, REVISIONS);
    erase.finish().await;
}

#[tokio::test]
async fn remaining_never_grows_survives_a_restart_and_reaches_zero_only_after_the_sweeper() {
    // Arrange
    let mut erase = Erase::new().await;
    let seeded = erase.seed().await;
    erase.mark_deleting().await;
    let start = probe(&erase).await;

    // Act
    let mut reported = Vec::new();
    let mut probes = Vec::new();
    for batch in 0..MAX_BATCHES {
        if batch == 5 {
            erase.restart().await;
        }
        reported.push(erase.erase(LIMIT).await);
        let after = probe(&erase).await;
        let done = after.nodes + after.revisions == 0;
        probes.push(after);
        if done {
            break;
        }
    }
    let files_before_sweep = seeded
        .blobs
        .iter()
        .filter(|&&id| erase.has_file(id))
        .count();
    let removed = erase.sweep().await;
    reported.push(erase.erase(LIMIT).await);
    let repeat = erase.erase(LIMIT).await;
    let stranger = erase
        .hook
        .erase_batch(Uuid::new_v4(), LIMIT)
        .await
        .expect("erase of an unknown account")
        .remaining;

    // Assert
    assert_eq!(
        (start.nodes, start.revisions, start.queued),
        (NODES, REVISIONS, 0)
    );
    assert!(probes.len() > 5, "the restart happened between batches");
    for (remaining, after) in reported.iter().zip(&probes) {
        assert_eq!(
            *remaining as i64,
            after.nodes + after.revisions + after.queued
        );
        assert!(
            after.root || after.nodes == 0,
            "root deleted before its tree"
        );
    }
    assert!(reported.windows(2).all(|pair| pair[1] <= pair[0]));
    assert_eq!(reported[probes.len() - 1], REVISIONS as u64);
    assert_eq!(files_before_sweep, seeded.blobs.len());
    assert_eq!(removed, REVISIONS as u64);
    assert_eq!(reported.last(), Some(&0));
    assert_eq!((repeat, stranger), (0, 0));
    assert!(seeded.blobs.iter().all(|&id| !erase.has_file(id)));
    assert_eq!(erase.of_account("blobs").await, 0);
    erase.finish().await;
}
