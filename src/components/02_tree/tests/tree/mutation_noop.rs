//! AZ-3708 review decision (owner): an `update_node` with no move and no new spelling returns the node as it is,
//! with no write, no stamp and no notify; the guard still runs first, and `NotFound`/`IsRoot` are unchanged.

use contracts::{NodeView, TreeServiceError};
use uuid::Uuid;

use crate::mutation_support::Tree;

/// A folder created through the service, so its `seq` and `updated_at` are the service's own.
async fn created(tree: &Tree, name: &str) -> NodeView {
    tree.service
        .create_folder(tree.account, tree.root, name)
        .await
        .expect("create")
}

async fn row_unchanged(tree: &Tree, before: &NodeView) -> bool {
    tree.count(&format!(
        "SELECT count(*) FROM nodes WHERE id = '{}' AND name = '{}' AND seq = {} \
         AND updated_at = 'epoch'::timestamptz + {}::bigint * interval '1 microsecond'",
        before.id,
        before.name,
        before.seq,
        before.updated_at.as_unix_micros()
    ))
    .await
        == 1
}

#[tokio::test]
async fn an_empty_update_returns_the_node_with_no_write_stamp_or_wake() {
    // Arrange
    let tree = Tree::new().await;
    let folder = created(&tree, "Docs").await;
    let wakes = tree.notifier.subscribe(tree.account);
    let seq = tree.seq().await;

    // Act
    let view = tree
        .service
        .update_node(tree.account, folder.id, None, None)
        .await
        .expect("an empty update succeeds");

    // Assert
    assert_eq!(view, folder, "the node as it is, seq unchanged");
    assert_eq!(tree.seq().await, seq, "no seq consumed");
    assert!(row_unchanged(&tree, &folder).await, "no write");
    assert_eq!(wakes.notified_seq(), 0, "no notify");
    drop(wakes);
    tree.finish().await;
}

#[tokio::test]
async fn a_rename_to_the_exact_same_name_is_the_same_no_op() {
    // Arrange
    let tree = Tree::new().await;
    let folder = created(&tree, "Docs").await;
    let wakes = tree.notifier.subscribe(tree.account);
    let seq = tree.seq().await;

    // Act
    let view = tree
        .service
        .update_node(tree.account, folder.id, Some("Docs"), None)
        .await
        .expect("a same-name rename succeeds");

    // Assert
    assert_eq!(view, folder);
    assert_eq!(tree.seq().await, seq);
    assert!(row_unchanged(&tree, &folder).await);
    assert_eq!(wakes.notified_seq(), 0);
    drop(wakes);
    tree.finish().await;
}

#[tokio::test]
async fn a_case_only_rename_is_still_a_change() {
    // Arrange
    let tree = Tree::new().await;
    let folder = created(&tree, "Docs").await;
    let seq = tree.seq().await;

    // Act
    let view = tree
        .service
        .update_node(tree.account, folder.id, Some("DOCS"), None)
        .await
        .expect("a case-only rename succeeds");

    // Assert
    assert_eq!((view.id, view.name.as_str()), (folder.id, "DOCS"));
    assert_eq!(view.seq, seq + 1);
    assert_eq!(tree.seq().await, seq + 1);
    tree.finish().await;
}

#[tokio::test]
async fn an_empty_update_still_answers_inactive_account_not_found_and_root() {
    // Arrange
    let tree = Tree::new().await;
    let folder = created(&tree, "Docs").await;
    let unknown = tree
        .service
        .update_node(tree.account, Uuid::new_v4(), None, None)
        .await;
    let root = tree
        .service
        .update_node(tree.account, tree.root, None, None)
        .await;
    tree.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        tree.account
    ))
    .await;
    let seq = tree.seq().await;

    // Act
    let inactive = tree
        .service
        .update_node(tree.account, folder.id, Some("Docs"), None)
        .await;

    // Assert
    assert!(
        matches!(inactive, Err(TreeServiceError::AccountNotActive)),
        "{inactive:?}"
    );
    assert!(
        matches!(unknown, Err(TreeServiceError::NotFound)),
        "{unknown:?}"
    );
    assert!(matches!(root, Err(TreeServiceError::IsRoot)), "{root:?}");
    assert_eq!(tree.seq().await, seq);
    tree.finish().await;
}
