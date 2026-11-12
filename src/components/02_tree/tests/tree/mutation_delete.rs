//! AZ-3708 AC-6 (delete rules, IT-02-4 without the quota half), AC-7 (one stamp per success, none per failure,
//! wake after COMMIT) and AC-8 (the account guard runs first and refuses an inactive account).

use contracts::TreeServiceError;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::support::{RevisionRow, blob, file_with_revision, revision};

/// File f with revisions r1 then r2, r2 current; returns (f, r1, r2).
async fn file_at_r2(tree: &Tree) -> (Uuid, Uuid, Uuid) {
    let mut owner = tree.fixture.owner_conn().await;
    let (f, r1, _) = file_with_revision(&mut owner, tree.account, tree.root, "f.txt").await;
    let (blob_id, r2) = (Uuid::new_v4(), Uuid::new_v4());
    blob(&mut owner, blob_id, tree.account).await;
    revision(
        &mut owner,
        &RevisionRow {
            id: r2,
            ..RevisionRow::new(tree.account, f, blob_id)
        },
    )
    .await
    .expect("insert r2");
    close(owner).await;
    tree.exec(&format!(
        "UPDATE nodes SET current_revision_id = '{r2}' WHERE id = '{f}'"
    ))
    .await;
    (f, r1, r2)
}

#[tokio::test]
async fn a_stale_base_is_refused_the_current_base_deletes_and_a_repeat_changes_nothing() {
    // Arrange
    let tree = Tree::new().await;
    let (f, r1, r2) = file_at_r2(&tree).await;
    let start = tree.seq().await;

    // Act
    let stale = tree.service.delete_node(tree.account, f, Some(r1)).await;
    let after_stale = tree.seq().await;
    let current = tree.service.delete_node(tree.account, f, Some(r2)).await;
    let after_delete = tree.seq().await;
    let repeat = tree.service.delete_node(tree.account, f, Some(r2)).await;

    // Assert
    assert!(
        matches!(stale, Err(TreeServiceError::EditedSince { current_revision }) if current_revision == r2),
        "{stale:?}"
    );
    assert_eq!(after_stale, start, "the refusal consumed no seq");
    current.expect("the current base deletes");
    assert_eq!(after_delete, start + 1, "one seq for the delete");
    repeat.expect("a repeat succeeds");
    assert_eq!(tree.seq().await, after_delete, "the repeat stamps nothing");
    let tombstoned = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{f}' AND deleted_at IS NOT NULL AND NOT cascade_pending \
             AND seq = {after_delete}"
        ))
        .await;
    assert_eq!(tombstoned, 1);
    tree.finish().await;
}

#[tokio::test]
async fn a_folder_delete_writes_one_tombstone_with_cascade_pending() {
    // Arrange
    let tree = Tree::new().await;
    let folder = tree.folder(tree.root, "F").await;
    let mut owner = tree.fixture.owner_conn().await;
    for index in 0..100 {
        file_with_revision(&mut owner, tree.account, folder, &format!("file{index}")).await;
    }
    close(owner).await;
    let seq = tree.seq().await;

    // Act
    tree.service
        .delete_node(tree.account, folder, None)
        .await
        .expect("delete F");

    // Assert
    let tombstones = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{}' AND deleted_at IS NOT NULL",
            tree.account
        ))
        .await;
    let marked = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{folder}' AND cascade_pending AND seq = {}",
            seq + 1
        ))
        .await;
    let live_children = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE parent_id = '{folder}' AND deleted_at IS NULL"
        ))
        .await;
    assert_eq!(tombstones, 1, "one tombstone");
    assert_eq!(marked, 1, "cascade_pending set and stamped with one seq");
    assert_eq!(live_children, 100, "descendants are left to the cascade");
    assert_eq!(tree.seq().await, seq + 1);
    tree.finish().await;
}

#[tokio::test]
async fn deleting_the_root_or_an_unknown_node_is_refused() {
    // Arrange
    let tree = Tree::new().await;
    let seq = tree.seq().await;

    // Act
    let root = tree
        .service
        .delete_node(tree.account, tree.root, None)
        .await;
    let unknown = tree
        .service
        .delete_node(tree.account, Uuid::new_v4(), None)
        .await;

    // Assert
    assert!(matches!(root, Err(TreeServiceError::IsRoot)), "{root:?}");
    assert!(
        matches!(unknown, Err(TreeServiceError::NotFound)),
        "{unknown:?}"
    );
    assert_eq!(tree.seq().await, seq);
    tree.finish().await;
}

#[tokio::test]
async fn the_wake_up_carries_the_committed_seq_and_a_failure_wakes_nobody() {
    // Arrange
    let tree = Tree::new().await;
    let mut wakes = tree.notifier.subscribe(tree.account);
    let refused = tree
        .service
        .create_folder(tree.account, Uuid::new_v4(), "x")
        .await;
    let silent = wakes.notified_seq();

    // Act
    let view = tree
        .service
        .create_folder(tree.account, tree.root, "x")
        .await
        .expect("create");
    wakes.notified().await;

    // Assert
    assert!(refused.is_err());
    assert_eq!(silent, 0, "a failure notifies nothing");
    assert_eq!(wakes.notified_seq(), view.seq);
    let visible = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{}' AND seq = {}",
            view.id, view.seq
        ))
        .await;
    assert_eq!(visible, 1, "the woken reader sees the committed row");
    drop(wakes);
    tree.finish().await;
}

#[tokio::test]
async fn the_guard_runs_before_any_row_is_written_or_locked() {
    // Arrange
    let tree = Tree::new().await;
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "a")
        .await
        .expect("create");

    // Act
    tree.service
        .update_node(tree.account, folder.id, Some("b"), Some(tree.root))
        .await
        .expect("update");
    tree.service
        .delete_node(tree.account, folder.id, None)
        .await
        .expect("delete");

    // Assert
    assert_eq!(tree.guard.calls(), [true, true, true]);
    tree.finish().await;
}

#[tokio::test]
async fn an_inactive_account_is_refused_by_every_mutation_with_nothing_written() {
    // Arrange
    let tree = Tree::new().await;
    let folder = tree.folder(tree.root, "F").await;
    tree.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        tree.account
    ))
    .await;
    let (nodes, seq) = (tree.nodes().await, tree.seq().await);

    // Act
    let create = tree
        .service
        .create_folder(tree.account, tree.root, "x")
        .await;
    let update = tree
        .service
        .update_node(tree.account, folder, Some("y"), Some(tree.root))
        .await;
    let delete = tree.service.delete_node(tree.account, folder, None).await;

    // Assert
    assert!(
        matches!(create, Err(TreeServiceError::AccountNotActive)),
        "{create:?}"
    );
    assert!(
        matches!(update, Err(TreeServiceError::AccountNotActive)),
        "{update:?}"
    );
    assert!(
        matches!(delete, Err(TreeServiceError::AccountNotActive)),
        "{delete:?}"
    );
    assert_eq!(tree.nodes().await, nodes, "0 rows written");
    assert_eq!(tree.seq().await, seq, "no seq consumed");
    let unchanged = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{folder}' AND name = 'F' AND deleted_at IS NULL"
        ))
        .await;
    assert_eq!(unchanged, 1);
    tree.finish().await;
}
