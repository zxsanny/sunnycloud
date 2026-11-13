//! AZ-3712 (ST-02-4): `publish` writes a revision only over a blob of the committing account. Another account's
//! blob and a random id fail the entry with the same `BlocklistMismatch`, the failure is per entry, and an own
//! blob still `uploading` (no size yet) is accepted.

use contracts::{EntryFailure, EntryResult, PublishOutcome, PublishResult, TreeCommit};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{actor, new_file};

fn results(result: PublishResult) -> Vec<EntryResult> {
    match result {
        PublishResult::Published(results) => results,
        PublishResult::Replayed(_) => panic!("unexpected replay"),
    }
}

fn outcome(result: &EntryResult) -> Option<PublishOutcome> {
    match result {
        EntryResult::Published { outcome, .. } => Some(*outcome),
        EntryResult::Failed(_) => None,
    }
}

/// 1 while the blob row is still the owner's committed blob of size 1 (as `Tree::blobs` wrote it).
async fn blob_row(tree: &Tree, blob: Uuid) -> i64 {
    tree.count(&format!(
        "SELECT count(*) FROM blobs WHERE id = '{blob}' AND account_id = '{}' \
         AND state = 'committed' AND size = 1",
        tree.account
    ))
    .await
}

async fn rows_of(tree: &Tree, table: &str, account: Uuid) -> i64 {
    tree.count(&format!(
        "SELECT count(*) FROM {table} WHERE account_id = '{account}'"
    ))
    .await
}

async fn seq_of(tree: &Tree, account: Uuid) -> i64 {
    tree.count(&format!(
        "SELECT seq FROM account_feeds WHERE account_id = '{account}'"
    ))
    .await
}

#[tokio::test]
async fn another_accounts_blob_is_refused_and_nothing_is_written() {
    // Arrange
    let tree = Tree::new().await;
    let theirs = tree.blobs(1).await[0];
    let (intruder, intruder_root) = tree.add_account().await;
    let nodes_before = rows_of(&tree, "nodes", intruder).await;

    // Act
    let result = tree
        .service
        .publish(
            actor(intruder),
            vec![new_file(intruder_root, "stolen.bin", theirs)],
            Vec::new(),
            sc_testdb::START,
        )
        .await
        .expect("the call itself succeeds");

    // Assert
    assert_eq!(
        results(result),
        [EntryResult::Failed(EntryFailure::BlocklistMismatch)]
    );
    assert_eq!(rows_of(&tree, "nodes", intruder).await, nodes_before);
    assert_eq!(rows_of(&tree, "revisions", intruder).await, 0);
    assert_eq!(
        blob_row(&tree, theirs).await,
        1,
        "the blob row is unchanged"
    );
    let pointing = tree
        .count(&format!(
            "SELECT count(*) FROM revisions WHERE blob_id = '{theirs}'"
        ))
        .await;
    assert_eq!(pointing, 0, "no revision reads the blob");
    tree.finish().await;
}

#[tokio::test]
async fn a_foreign_blob_and_a_random_blob_id_give_the_same_entry_error() {
    // Arrange
    let tree = Tree::new().await;
    let theirs = tree.blobs(1).await[0];
    let (intruder, intruder_root) = tree.add_account().await;
    let entries = vec![
        new_file(intruder_root, "foreign.bin", theirs),
        new_file(intruder_root, "random.bin", Uuid::new_v4()),
    ];

    // Act
    let result = tree
        .service
        .publish(actor(intruder), entries, Vec::new(), sc_testdb::START)
        .await
        .expect("the call itself succeeds");

    // Assert
    let results = results(result);
    assert_eq!(results[0], results[1]);
    assert_eq!(
        results[0],
        EntryResult::Failed(EntryFailure::BlocklistMismatch)
    );
    tree.finish().await;
}

#[tokio::test]
async fn a_foreign_blob_in_the_middle_of_a_batch_fails_only_its_entry_under_one_seq() {
    // Arrange
    let tree = Tree::new().await;
    let theirs = tree.blobs(1).await[0];
    let (intruder, intruder_root) = tree.add_account().await;
    let own = tree.blobs_of(intruder, 2, "committed").await;
    let entries = vec![
        new_file(intruder_root, "first.bin", own[0]),
        new_file(intruder_root, "foreign.bin", theirs),
        new_file(intruder_root, "third.bin", own[1]),
    ];
    let seq_before = seq_of(&tree, intruder).await;

    // Act
    let result = tree
        .service
        .publish(actor(intruder), entries, Vec::new(), sc_testdb::START)
        .await
        .expect("the call itself succeeds");

    // Assert
    let results = results(result);
    assert_eq!(
        results.iter().map(outcome).collect::<Vec<_>>(),
        [
            Some(PublishOutcome::Created),
            None,
            Some(PublishOutcome::Created)
        ]
    );
    assert_eq!(
        results[1],
        EntryResult::Failed(EntryFailure::BlocklistMismatch)
    );
    assert_eq!(seq_of(&tree, intruder).await, seq_before + 1);
    assert_eq!(rows_of(&tree, "revisions", intruder).await, 2);
    tree.finish().await;
}

#[tokio::test]
async fn an_own_blob_still_uploading_is_published() {
    // Arrange
    let tree = Tree::new().await;
    let uploading = tree.blobs_of(tree.account, 1, "uploading").await[0];

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![new_file(tree.root, "mine.bin", uploading)],
            Vec::new(),
            sc_testdb::START,
        )
        .await
        .expect("publish");

    // Assert
    assert_eq!(
        results(result).iter().map(outcome).collect::<Vec<_>>(),
        [Some(PublishOutcome::Created)]
    );
    tree.finish().await;
}
