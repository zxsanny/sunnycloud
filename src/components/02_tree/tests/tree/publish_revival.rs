//! AZ-3710 AC-4 (IT-02-8: an edit beats a delete; the file and its tombstoned ancestors are revived at one seq,
//! `cascade_pending` cleared, marked siblings stay deleted), the revive before the cascade has marked everything
//! (owner decision 2026-11-13: unmarked descendants come back live, marked ones stay deleted) and AC-5 (a name
//! taken meanwhile gives the revived node a conflict-copy name).

use contracts::{Cursor, NodeState, PublishOutcome, TreeMaintenance};
use uuid::Uuid;

use crate::conflict_support::{Done, publish_as};
use crate::maintenance_support::{Maint, config};
use crate::mutation_support::Tree;
use crate::publish_support::{existing, new_file};
use crate::support::START;

/// `R/A/B/f` and `R/A/B/g`, each file with one revision: `(A, B, f, g)`.
async fn subtree(tree: &Tree, blobs: &[Uuid]) -> (Uuid, Uuid, Done, Done) {
    let a = tree.folder(tree.root, "A").await;
    let b = tree.folder(a, "B").await;
    let f = publish_as(tree, "laptop", new_file(b, "f", blobs[0])).await;
    let g = publish_as(tree, "laptop", new_file(b, "g", blobs[1])).await;
    (a, b, f, g)
}

async fn delete(tree: &Tree, node: Uuid) {
    tree.service
        .delete_node(tree.account, node, None)
        .await
        .expect("delete");
}

#[tokio::test]
async fn an_edit_of_a_cascaded_file_revives_it_and_its_ancestors_at_one_seq_and_leaves_its_sibling_deleted()
 {
    // Arrange
    let maint = Maint::new(config(100, 10)).await;
    let tree = maint.tree();
    let blobs = tree.blobs(3).await;
    let (a, b, f, g) = subtree(tree, &blobs).await;
    delete(tree, a).await;
    assert_eq!(maint.cascade_until_idle(START, 100).await, [3, 0]);
    let (nodes, seq) = (tree.nodes().await, tree.seq().await);

    // Act
    let edit = publish_as(tree, "laptop", existing(f.node, Some(f.revision), blobs[2])).await;

    // Assert
    assert_eq!(
        (edit.outcome, edit.node, edit.conflict_copy),
        (PublishOutcome::Revived, f.node, None)
    );
    for node in [a, b, f.node] {
        let row = tree.row(node).await;
        assert!(row.live && !row.cascade_pending, "{node}: {row:?}");
    }
    assert!(
        !tree.row(g.node).await.live,
        "g was marked: it stays deleted"
    );
    assert_eq!(tree.row(f.node).await.current, Some(edit.revision));
    assert_eq!(tree.nodes().await, nodes, "no conflict copy");
    let page = maint
        .reader()
        .read_page(tree.account, Cursor { epoch: 1, seq }, None)
        .await
        .expect("feed page");
    let mut changed: Vec<(Uuid, NodeState, i64)> = page
        .items
        .iter()
        .map(|item| (item.node_id, item.state, item.seq))
        .collect();
    changed.sort_by_key(|change| change.0);
    let mut expected = vec![
        (a, NodeState::Live, seq + 1),
        (b, NodeState::Live, seq + 1),
        (f.node, NodeState::Live, seq + 1),
    ];
    expected.sort_by_key(|change| change.0);
    assert_eq!(changed, expected, "A and B live again with f, one seq");
    let f_item = page.items.iter().find(|item| item.node_id == f.node);
    assert_eq!(
        f_item.and_then(|item| item.revision_id),
        Some(edit.revision)
    );
    maint.finish().await;
}

#[tokio::test]
async fn reviving_before_the_cascade_is_done_brings_back_unmarked_descendants_and_keeps_marked_ones_deleted()
 {
    // Arrange
    let maint = Maint::new(config(100, 10)).await;
    let tree = maint.tree();
    let blobs = tree.blobs(4).await;
    let (a, b, f, g) = subtree(tree, &blobs).await;
    let h = publish_as(tree, "laptop", new_file(a, "h", blobs[2])).await;
    delete(tree, a).await;
    let marked = maint
        .maintenance
        .cascade_batch(START, 2)
        .await
        .expect("cascade batch");
    assert_eq!(marked, 2, "B and h marked; f and g not yet");
    assert!(tree.row(b).await.cascade_pending && tree.row(f.node).await.live);

    // Act
    let edit = publish_as(tree, "laptop", existing(f.node, Some(f.revision), blobs[3])).await;
    let later = maint.cascade_until_idle(START, 100).await;

    // Assert
    assert_eq!(edit.outcome, PublishOutcome::Revived);
    for node in [a, b] {
        let row = tree.row(node).await;
        assert!(row.live && !row.cascade_pending, "{node}: {row:?}");
    }
    assert!(tree.row(f.node).await.live);
    assert!(tree.row(g.node).await.live, "g was not marked: it is back");
    assert!(
        !tree.row(h.node).await.live,
        "h was marked: it stays deleted"
    );
    assert_eq!(later, [0], "the cascade has nothing left to mark");
    maint.finish().await;
}

#[tokio::test]
async fn a_revived_file_whose_name_was_taken_meanwhile_gets_a_conflict_copy_name() {
    // Arrange
    let maint = Maint::new(config(100, 10)).await;
    let tree = maint.tree();
    let blobs = tree.blobs(4).await;
    let (_, b, f, _) = subtree(tree, &blobs).await;
    delete(tree, f.node).await;
    let newer = publish_as(tree, "laptop", new_file(b, "f", blobs[2])).await;
    assert_eq!(newer.outcome, PublishOutcome::Created);

    // Act
    let edit = publish_as(tree, "laptop", existing(f.node, Some(f.revision), blobs[3])).await;

    // Assert
    assert_eq!(
        (edit.outcome, edit.conflict_copy),
        (PublishOutcome::CreatedAsCopy { node: f.node }, Some(f.node))
    );
    let revived = tree.row(f.node).await;
    assert_eq!(
        (revived.name.as_str(), revived.live, revived.current),
        (
            "f (conflict copy 2026-11-07 12-00 laptop)",
            true,
            Some(edit.revision)
        )
    );
    let untouched = tree.row(newer.node).await;
    assert_eq!(
        (untouched.name.as_str(), untouched.current),
        ("f", Some(newer.revision))
    );
    maint.finish().await;
}

#[tokio::test]
async fn a_revived_ancestor_whose_name_was_taken_meanwhile_gets_a_conflict_copy_name() {
    // Arrange
    let maint = Maint::new(config(100, 10)).await;
    let tree = maint.tree();
    let blobs = tree.blobs(3).await;
    let (a, b, f, _) = subtree(tree, &blobs).await;
    delete(tree, a).await;
    maint.cascade_until_idle(START, 100).await;
    let newer = tree.folder(tree.root, "A").await;

    // Act
    let edit = publish_as(tree, "laptop", existing(f.node, Some(f.revision), blobs[2])).await;

    // Assert
    assert_eq!(edit.outcome, PublishOutcome::Revived, "f keeps its name");
    let revived = tree.row(a).await;
    assert_eq!(
        (revived.name.as_str(), revived.live),
        ("A (conflict copy 2026-11-07 12-00 laptop)", true)
    );
    assert_eq!(tree.row(newer).await.name, "A");
    assert_eq!(
        (tree.row(b).await.name.as_str(), tree.row(f.node).await.live),
        ("B", true)
    );
    maint.finish().await;
}
