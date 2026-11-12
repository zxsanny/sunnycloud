//! AZ-3708 AC-3 (parent errors on create, IT-02-3) and AC-4 (rename and move rules), with the feed delta of
//! AC-7 on every call.

use std::time::{Duration, Instant};

use contracts::{NodeKind, NodeView, TreeServiceError};
use uuid::Uuid;

use crate::mutation_support::{Tree, close, feed_row};
use crate::support::{account_with_root, file_with_revision, node};

/// `R/A/B/f.txt`: the ids of A, B and f, and the current revision of f.
struct Sample {
    a: Uuid,
    b: Uuid,
    f: Uuid,
    revision: Uuid,
}

async fn sample(tree: &Tree) -> Sample {
    let a = tree.folder(tree.root, "A").await;
    let b = tree.folder(a, "B").await;
    let mut owner = tree.fixture.owner_conn().await;
    let (f, revision, _) = file_with_revision(&mut owner, tree.account, b, "f.txt").await;
    close(owner).await;
    Sample { a, b, f, revision }
}

/// Runs a failing call and returns its error after checking that it wrote nothing and consumed no `seq`.
async fn refused<T: std::fmt::Debug>(
    tree: &Tree,
    call: impl Future<Output = Result<T, TreeServiceError>>,
) -> TreeServiceError {
    let (nodes, seq) = (tree.nodes().await, tree.seq().await);
    let error = call.await.expect_err("the call is refused");
    assert_eq!(tree.nodes().await, nodes, "0 rows written for {error:?}");
    assert_eq!(tree.seq().await, seq, "no seq consumed for {error:?}");
    error
}

/// Runs a succeeding call and checks it advanced the feed by exactly one.
async fn stamped_once(
    tree: &Tree,
    call: impl Future<Output = Result<NodeView, TreeServiceError>>,
) -> NodeView {
    let seq = tree.seq().await;
    let view = call.await.expect("the call succeeds");
    assert_eq!(tree.seq().await, seq + 1, "one seq per success");
    assert_eq!(view.seq, seq + 1, "the view carries the stamped seq");
    view
}

#[tokio::test]
async fn create_folder_under_each_unusable_parent_is_refused() {
    // Arrange
    let tree = Tree::new().await;
    let gone = tree.folder(tree.root, "gone").await;
    tree.tombstone(gone).await;
    let mut owner = tree.fixture.owner_conn().await;
    let (file, _, _) = file_with_revision(&mut owner, tree.account, tree.root, "f.txt").await;
    let other = Uuid::new_v4();
    let other_root = account_with_root(&mut owner, other).await;
    feed_row(&mut owner, other).await;
    let foreign = node(&mut owner, other, Some(other_root), "folder", "theirs")
        .await
        .expect("insert foreign folder");
    close(owner).await;
    let service = &tree.service;

    // Act
    let random = refused(
        &tree,
        service.create_folder(tree.account, Uuid::new_v4(), "x"),
    )
    .await;
    let deleted = refused(&tree, service.create_folder(tree.account, gone, "x")).await;
    let not_folder = refused(&tree, service.create_folder(tree.account, file, "x")).await;
    let foreign = refused(&tree, service.create_folder(tree.account, foreign, "x")).await;

    // Assert
    assert!(matches!(random, TreeServiceError::NotFound), "{random:?}");
    assert!(
        matches!(deleted, TreeServiceError::ParentDeleted),
        "{deleted:?}"
    );
    assert!(
        matches!(not_folder, TreeServiceError::NotAFolder),
        "{not_folder:?}"
    );
    assert!(matches!(foreign, TreeServiceError::NotFound), "{foreign:?}");
    tree.finish().await;
}

#[tokio::test]
async fn a_case_only_rename_keeps_the_node_id_with_the_new_spelling() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;

    // Act
    let view = stamped_once(
        &tree,
        tree.service
            .update_node(tree.account, s.f, Some("F.TXT"), None),
    )
    .await;

    // Assert
    assert_eq!(view.id, s.f);
    assert_eq!(view.name, "F.TXT");
    assert_eq!(view.parent_id, Some(s.b));
    assert_eq!(view.kind, NodeKind::File);
    assert_eq!(view.current_revision_id, Some(s.revision));
    let stored = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{}' AND name = 'F.TXT' AND name_key = 'f.txt'",
            s.f
        ))
        .await;
    assert_eq!(stored, 1);
    tree.finish().await;
}

#[tokio::test]
async fn a_rename_and_move_together_and_a_sibling_move_succeed() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;
    let sibling = tree.folder(tree.root, "Sibling").await;

    // Act
    let moved = stamped_once(
        &tree,
        tree.service
            .update_node(tree.account, s.b, Some("Moved"), Some(sibling)),
    )
    .await;
    let back = stamped_once(
        &tree,
        tree.service
            .update_node(tree.account, sibling, None, Some(s.a)),
    )
    .await;

    // Assert
    assert_eq!(
        (moved.parent_id, moved.name.as_str()),
        (Some(sibling), "Moved")
    );
    assert_eq!((back.parent_id, back.name.as_str()), (Some(s.a), "Sibling"));
    tree.finish().await;
}

#[tokio::test]
async fn moves_that_would_close_a_cycle_are_refused() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;

    // Act
    let into_child = refused(
        &tree,
        tree.service.update_node(tree.account, s.a, None, Some(s.b)),
    )
    .await;
    let into_self = refused(
        &tree,
        tree.service.update_node(tree.account, s.a, None, Some(s.a)),
    )
    .await;

    // Assert
    assert!(
        matches!(into_child, TreeServiceError::Cycle),
        "{into_child:?}"
    );
    assert!(
        matches!(into_self, TreeServiceError::Cycle),
        "{into_self:?}"
    );
    tree.finish().await;
}

#[tokio::test]
async fn a_move_into_a_descendant_30_levels_deep_is_a_cycle_found_within_100_ms() {
    // Arrange
    let tree = Tree::new().await;
    let top = tree.folder(tree.root, "top").await;
    let mut deepest = top;
    for level in 1..=30 {
        deepest = tree.folder(deepest, &format!("level{level}")).await;
    }

    // Act
    let started = Instant::now();
    let outcome = tree
        .service
        .update_node(tree.account, top, None, Some(deepest))
        .await;
    let elapsed = started.elapsed();

    // Assert
    assert!(
        matches!(outcome, Err(TreeServiceError::Cycle)),
        "{outcome:?}"
    );
    assert!(
        elapsed < Duration::from_millis(100),
        "the deep check took {elapsed:?}"
    );
    tree.finish().await;
}

#[tokio::test]
async fn the_root_cannot_be_renamed_or_moved() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;

    // Act
    let renamed = refused(
        &tree,
        tree.service
            .update_node(tree.account, tree.root, Some("R"), None),
    )
    .await;
    let moved = refused(
        &tree,
        tree.service
            .update_node(tree.account, tree.root, None, Some(s.a)),
    )
    .await;

    // Assert
    assert!(matches!(renamed, TreeServiceError::IsRoot), "{renamed:?}");
    assert!(matches!(moved, TreeServiceError::IsRoot), "{moved:?}");
    tree.finish().await;
}

#[tokio::test]
async fn moves_into_a_tombstone_onto_a_taken_name_and_into_a_file_are_refused() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;
    let gone = tree.folder(tree.root, "gone").await;
    tree.tombstone(gone).await;
    tree.folder(tree.root, "b").await;
    let service = &tree.service;

    // Act
    let into_tombstone = refused(
        &tree,
        service.update_node(tree.account, s.b, None, Some(gone)),
    )
    .await;
    let onto_taken = refused(
        &tree,
        service.update_node(tree.account, s.b, None, Some(tree.root)),
    )
    .await;
    let into_file = refused(
        &tree,
        service.update_node(tree.account, s.b, None, Some(s.f)),
    )
    .await;
    let unknown = refused(
        &tree,
        service.update_node(tree.account, Uuid::new_v4(), Some("x"), None),
    )
    .await;

    // Assert
    assert!(
        matches!(into_tombstone, TreeServiceError::ParentDeleted),
        "{into_tombstone:?}"
    );
    assert!(
        matches!(onto_taken, TreeServiceError::NameConflict),
        "{onto_taken:?}"
    );
    assert!(
        matches!(into_file, TreeServiceError::NotAFolder),
        "{into_file:?}"
    );
    assert!(matches!(unknown, TreeServiceError::NotFound), "{unknown:?}");
    tree.finish().await;
}

#[tokio::test]
async fn a_rename_onto_a_taken_name_is_a_conflict_and_an_invalid_name_is_refused() {
    // Arrange
    let tree = Tree::new().await;
    let s = sample(&tree).await;
    tree.folder(s.a, "Other").await;

    // Act
    let taken = refused(
        &tree,
        tree.service
            .update_node(tree.account, s.b, Some("OTHER"), None),
    )
    .await;
    let invalid = refused(
        &tree,
        tree.service
            .update_node(tree.account, s.b, Some(".."), None),
    )
    .await;

    // Assert
    assert!(matches!(taken, TreeServiceError::NameConflict), "{taken:?}");
    assert!(
        matches!(invalid, TreeServiceError::NameInvalid(_)),
        "{invalid:?}"
    );
    tree.finish().await;
}
