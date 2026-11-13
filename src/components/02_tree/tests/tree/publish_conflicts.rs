//! AZ-3710 AC-1 and AC-2 (IT-02-6: a stale base keeps the superseded revision row in a conflict copy; collision
//! suffix and a 250-byte stem) and AC-3 (IT-02-7: a taken name on a new file answers `CreatedAsCopy`, also for
//! concurrent commits and for a race lost on the name index).

use std::sync::Arc;
use std::time::Duration;

use contracts::PublishOutcome;
use sc_clock::{ManualClock, Timestamp};
use sqlx::Connection;
use uuid::Uuid;

use crate::conflict_support::{Done, publish_as};
use crate::mutation_support::{Tree, close};
use crate::publish_support::{existing, new_file};
use crate::support::node;

/// 2026-11-04 09:07:30 UTC.
const AT_0907: Timestamp = Timestamp::from_unix_micros(1_793_783_250_000_000);

const WAITING_ON_A_TRANSACTION: &str = "SELECT count(*) FROM pg_locks \
    WHERE locktype = 'transactionid' AND NOT granted \
    AND pid IN (SELECT pid FROM pg_stat_activity WHERE datname = current_database())";
const POLL: Duration = Duration::from_millis(10);
const POLL_LIMIT: u32 = 500;

/// A file `name` with r1, then r2 published by `laptop` on base r1: `(node, r1, r2)`.
async fn edited_once(tree: &Tree, name: &str, blobs: &[Uuid]) -> (Uuid, Uuid, Uuid) {
    let created = publish_as(tree, "laptop", new_file(tree.root, name, blobs[0])).await;
    let updated = publish_as(
        tree,
        "laptop",
        existing(created.node, Some(created.revision), blobs[1]),
    )
    .await;
    assert_eq!(updated.outcome, PublishOutcome::Updated);
    (created.node, created.revision, updated.revision)
}

fn copy_node(done: &Done) -> Uuid {
    match done.outcome {
        PublishOutcome::ConflictCopied { copy_node } => copy_node,
        other => panic!("expected ConflictCopied, got {other:?}"),
    }
}

#[tokio::test]
async fn a_stale_base_makes_the_new_revision_current_and_moves_the_superseded_row_to_a_conflict_copy()
 {
    // Arrange
    let tree = Tree::with_clock(Arc::new(ManualClock::new(AT_0907))).await;
    let blobs = tree.blobs(3).await;
    let (n, r1, r2) = edited_once(&tree, "report.docx", &blobs).await;
    let revisions = tree.revisions().await;
    let seq = tree.seq().await;

    // Act
    let stale = publish_as(&tree, "phone-2", existing(n, Some(r1), blobs[2])).await;

    // Assert
    let copy = copy_node(&stale);
    assert_eq!((stale.node, stale.conflict_copy), (n, Some(copy)));
    let file = tree.row(n).await;
    assert_eq!(file.current, Some(stale.revision), "r3 is current");
    let kept = tree.row(copy).await;
    assert_eq!(
        kept.name,
        "report (conflict copy 2026-11-04 09-07 phone-2).docx"
    );
    assert_eq!(
        (kept.parent, kept.live, kept.current),
        (Some(tree.root), true, Some(r2))
    );
    assert_eq!(
        tree.revision_row(r2).await,
        (copy, blobs[1]),
        "the r2 row itself, with its blob"
    );
    assert_eq!(tree.revisions().await, revisions + 1, "only r3 was added");
    assert_eq!(tree.seq().await, seq + 1);
    assert_eq!((file.seq, kept.seq), (seq + 1, seq + 1), "both at one seq");
    tree.finish().await;
}

#[tokio::test]
async fn a_taken_conflict_copy_name_gets_a_numeric_suffix() {
    // Arrange
    let tree = Tree::with_clock(Arc::new(ManualClock::new(AT_0907))).await;
    let blobs = tree.blobs(4).await;
    let (n, r1, _) = edited_once(&tree, "report.docx", &blobs).await;
    let taken = "report (conflict copy 2026-11-04 09-07 phone-2).docx";
    publish_as(&tree, "laptop", new_file(tree.root, taken, blobs[3])).await;

    // Act
    let stale = publish_as(&tree, "phone-2", existing(n, Some(r1), blobs[2])).await;

    // Assert
    assert_eq!(
        tree.row(copy_node(&stale)).await.name,
        "report (conflict copy 2026-11-04 09-07 phone-2) (2).docx"
    );
    tree.finish().await;
}

#[tokio::test]
async fn the_conflict_copy_of_a_250_byte_stem_is_at_most_255_bytes() {
    // Arrange
    let tree = Tree::with_clock(Arc::new(ManualClock::new(AT_0907))).await;
    let blobs = tree.blobs(3).await;
    let stem = "s".repeat(250);
    let (n, r1, _) = edited_once(&tree, &format!("{stem}.txt"), &blobs).await;

    // Act
    let stale = publish_as(&tree, "phone-2", existing(n, Some(r1), blobs[2])).await;

    // Assert
    let name = tree.row(copy_node(&stale)).await.name;
    assert!(name.len() <= 255, "{} bytes: {name}", name.len());
    assert!(
        name.starts_with("sss") && name.ends_with(".txt"),
        "stem cut, extension kept: {name}"
    );
    assert!(name.contains(" (conflict copy 2026-11-04 09-07"), "{name}");
    tree.finish().await;
}

#[tokio::test]
async fn a_new_file_whose_name_is_taken_is_created_as_a_copy_also_when_two_commit_concurrently() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(4).await;
    let original = publish_as(&tree, "laptop", new_file(tree.root, "A.txt", blobs[0])).await;

    // Act
    let first = publish_as(&tree, "laptop", new_file(tree.root, "a.txt", blobs[1])).await;
    let (second, third) = tokio::join!(
        publish_as(&tree, "laptop", new_file(tree.root, "a.txt", blobs[2])),
        publish_as(&tree, "laptop", new_file(tree.root, "a.txt", blobs[3])),
    );

    // Assert
    let mut names = Vec::new();
    for done in [first, second, third] {
        assert_eq!(
            (done.outcome, done.conflict_copy),
            (
                PublishOutcome::CreatedAsCopy { node: done.node },
                Some(done.node)
            )
        );
        names.push(tree.row(done.node).await.name);
    }
    names.sort();
    assert_eq!(
        names,
        [
            "a (conflict copy 2026-11-07 12-00 laptop) (2).txt",
            "a (conflict copy 2026-11-07 12-00 laptop) (3).txt",
            "a (conflict copy 2026-11-07 12-00 laptop).txt",
        ]
    );
    let kept = tree.row(original.node).await;
    assert_eq!(
        (kept.name.as_str(), kept.current),
        ("A.txt", Some(original.revision))
    );
    assert_eq!(tree.nodes().await, 5, "the root and four files");
    tree.finish().await;
}

#[tokio::test]
async fn a_conflict_copy_name_taken_by_a_concurrent_transaction_moves_on_to_the_next_free_name() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(2).await;
    publish_as(&tree, "laptop", new_file(tree.root, "A.txt", blobs[0])).await;
    let mut other = tree.fixture.owner_conn().await;
    let mut racing = other.begin().await.expect("begin the racing transaction");
    let raced = "a (conflict copy 2026-11-07 12-00 laptop).txt";
    let rival = node(&mut racing, tree.account, Some(tree.root), "folder", raced)
        .await
        .expect("insert the rival, not committed yet");

    // Act
    let (copy, ()) = tokio::join!(
        publish_as(&tree, "laptop", new_file(tree.root, "a.txt", blobs[1])),
        async {
            wait_for_a_blocked_insert(&tree).await;
            racing.commit().await.expect("commit the rival");
        },
    );

    // Assert
    assert_eq!(
        copy.outcome,
        PublishOutcome::CreatedAsCopy { node: copy.node }
    );
    assert_eq!(
        tree.row(copy.node).await.name,
        "a (conflict copy 2026-11-07 12-00 laptop) (2).txt"
    );
    assert_eq!(tree.row(rival).await.name, raced);
    close(other).await;
    tree.finish().await;
}

/// Waits until the publish's insert waits for the rival's transaction on the name index.
async fn wait_for_a_blocked_insert(tree: &Tree) {
    for _ in 0..POLL_LIMIT {
        if tree.count(WAITING_ON_A_TRANSACTION).await == 1 {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("the publish never waited for the racing transaction");
}
