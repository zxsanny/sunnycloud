//! AZ-3719 AC-7 (AT-02-1, AC-8): two devices edit `report.docx` from the same base; the later commit wins the
//! name and the earlier one is kept in exactly one conflict copy named after the device that committed late.

use contracts::{CommitActor, EntryResult, PublishResult, TreeCommit};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{existing, new_file};
use crate::reader_support::reader;

fn device(tree: &Tree, label: &str) -> CommitActor {
    CommitActor {
        account_id: tree.account,
        device_label: label.to_owned(),
    }
}

async fn commit(
    tree: &Tree,
    label: &str,
    entry: contracts::PublishEntry,
) -> (Uuid, contracts::PublishOutcome) {
    match tree
        .service
        .publish(
            device(tree, label),
            vec![entry],
            Vec::new(),
            sc_testdb::START,
        )
        .await
        .expect("publish")
    {
        PublishResult::Published(results) => match results.as_slice() {
            [
                EntryResult::Published {
                    revision_id,
                    outcome,
                    ..
                },
            ] => (*revision_id, *outcome),
            other => panic!("expected one published entry, got {other:?}"),
        },
        PublishResult::Replayed(_) => panic!("unexpected replay"),
    }
}

#[tokio::test]
async fn two_stale_edits_keep_both_versions_with_one_conflict_copy_named_for_the_late_device() {
    // Arrange
    let tree = Tree::new().await;
    let folder = tree.folder(tree.root, "work").await;
    let blobs = tree.blobs(3).await;
    let first = new_file(folder, "report.docx", blobs[0]);
    let (r1, _) = commit(&tree, "laptop", first).await;
    let report = reader(&tree, 1000)
        .find_live_node(tree.account, folder, "report.docx")
        .await
        .expect("find")
        .expect("report.docx")
        .id;

    // Act
    let (r2, laptop) = commit(&tree, "laptop", existing(report, Some(r1), blobs[1])).await;
    let (r3, phone) = commit(&tree, "phone", existing(report, Some(r1), blobs[2])).await;
    let listing = reader(&tree, 1000)
        .list_children(tree.account, folder, None, 0)
        .await
        .expect("listing");

    // Assert
    assert_eq!(laptop, contracts::PublishOutcome::Updated);
    assert!(matches!(
        phone,
        contracts::PublishOutcome::ConflictCopied { .. }
    ));
    let items: Vec<(String, Option<Uuid>)> = listing
        .items
        .into_iter()
        .map(|item| (item.name, item.current_revision_id))
        .collect();
    assert_eq!(
        items,
        [
            (
                "report (conflict copy 2026-11-07 12-00 phone).docx".to_owned(),
                Some(r2)
            ),
            ("report.docx".to_owned(), Some(r3)),
        ]
    );
    tree.finish().await;
}
