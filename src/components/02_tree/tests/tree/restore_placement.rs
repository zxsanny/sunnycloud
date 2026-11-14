//! AZ-3715 AC-1 to AC-3 and the revision choice (IT-02-21): the restored revision row is re-pointed, never
//! copied, into its revived original node or into the root folder `Restored`.

use contracts::{Placement, RestoreError, TreeRestore as _};
use uuid::Uuid;

use crate::restore_support::{Restore, hours_ago, micros_after};

#[tokio::test]
async fn the_latest_revision_at_or_before_the_instant_is_chosen() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    let blob3 = rig.blob_with_file().await;
    rig.publish_at(
        crate::usage_support::edit_sized(file.node, Some(file.r2), blob3, 7),
        hours_ago(1),
    )
    .await;

    // Act
    let at_r2 = rig.by_node(file.node, hours_ago(2)).await;
    let just_before_r2 = rig.by_node(file.node, micros_after(hours_ago(2), -1)).await;
    let before_r1 = rig.by_node(file.node, micros_after(hours_ago(3), -1)).await;

    // Assert
    assert_eq!(at_r2.expect("restore at r2").restored_revision, file.r2);
    assert_eq!(
        just_before_r2.expect("restore before r2").restored_revision,
        file.r1
    );
    assert!(matches!(before_r1, Err(RestoreError::NotFound)));
    rig.finish().await;
}

#[tokio::test]
async fn a_deleted_file_in_a_live_folder_is_revived_with_the_old_revision_and_no_new_row() {
    // Arrange
    let rig = Restore::new().await;
    let docs = rig.tree.folder(rig.tree.root, "docs").await;
    let file = rig.two_revisions(docs, "report.txt").await;
    rig.delete(file.node).await;
    let revisions = rig.tree.revisions().await;

    // Act
    let outcome = rig
        .by_path("docs/report.txt", micros_after(hours_ago(3), 1))
        .await
        .expect("restore");

    // Assert
    assert_eq!(outcome.placement, Placement::Original);
    assert_eq!(outcome.restored_revision, file.r1);
    assert_eq!(rig.tree.revisions().await, revisions);
    assert_eq!(
        rig.node(file.node).await,
        (Some(docs), "report.txt".to_owned(), true, Some(file.r1))
    );
    rig.finish().await;
}

#[tokio::test]
async fn files_of_deleted_folders_land_in_one_restored_root_folder_with_their_rows_reparented() {
    // Arrange
    let rig = Restore::new().await;
    let old = rig.tree.folder(rig.tree.root, "old").await;
    let older = rig.tree.folder(rig.tree.root, "older").await;
    let report = rig.two_revisions(old, "report.txt").await;
    let notes = rig.two_revisions(older, "notes.txt").await;
    for node in [report.node, notes.node, old, older] {
        rig.delete(node).await;
    }
    let revisions = rig.tree.revisions().await;
    let at = micros_after(hours_ago(3), 1);

    // Act
    let first = rig.by_path("old/report.txt", at).await.expect("restore 1");
    let second = rig.by_path("older/notes.txt", at).await.expect("restore 2");

    // Assert
    assert_eq!(
        (first.placement, second.placement),
        (Placement::Restored, Placement::Restored)
    );
    let restored_folders = rig
        .tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE parent_id = '{}' AND name = 'Restored' \
             AND kind = 'folder' AND deleted_at IS NULL",
            rig.tree.root
        ))
        .await;
    assert_eq!(restored_folders, 1);
    let (first_node, second_node) = (
        rig.revision_node(report.r1).await,
        rig.revision_node(notes.r1).await,
    );
    let (folder, name, live, current) = rig.node(first_node).await;
    assert_eq!(
        (name.as_str(), live, current),
        ("report.txt", true, Some(report.r1))
    );
    let (second_folder, second_name, ..) = rig.node(second_node).await;
    assert_eq!((second_folder, second_name.as_str()), (folder, "notes.txt"));
    assert_ne!(first_node, report.node);
    assert_eq!(rig.tree.revisions().await, revisions);
    rig.finish().await;
}

#[tokio::test]
async fn a_taken_name_gives_a_conflict_copy_and_the_restored_blob_leaves_the_queue() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "report.txt").await;
    rig.delete(file.node).await;
    let newcomer = rig.blob_with_file().await;
    let (taken_by, _) = rig
        .create_at(rig.tree.root, "report.txt", newcomer, 5, hours_ago(0))
        .await;
    assert!(
        rig.tree.gc_row(file.blob1).await.is_some(),
        "r1 queued when replaced"
    );

    // Act
    let outcome = rig
        .by_node(file.node, micros_after(hours_ago(3), 1))
        .await
        .expect("restore");

    // Assert
    assert_eq!(outcome.placement, Placement::Original);
    let (_, name, live, current) = rig.node(file.node).await;
    assert!(live);
    assert_eq!(current, Some(file.r1));
    assert!(
        name.starts_with("report (conflict copy ") && name.ends_with(").txt"),
        "{name}"
    );
    assert_eq!(rig.node(taken_by).await.1, "report.txt");
    assert!(
        rig.tree.gc_row(file.blob2).await.is_some(),
        "the previously current blob stays queued"
    );
    assert_eq!(rig.tree.gc_row(file.blob1).await, None);
    assert_eq!(rig.blob_state(file.blob1).await, "committed");
    rig.finish().await;
}

#[tokio::test]
async fn another_accounts_node_is_not_found_and_unchanged() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    rig.delete(file.node).await;
    let (other, _) = rig.tree.add_account().await;
    let before = rig.snapshot().await;

    // Act
    let foreign = rig
        .restore
        .restore_revision(
            other,
            contracts::NodeOrPath::Node(file.node),
            micros_after(hours_ago(3), 1),
        )
        .await;
    let random = rig
        .by_node(Uuid::new_v4(), micros_after(hours_ago(3), 1))
        .await;

    // Assert
    assert!(matches!(foreign, Err(RestoreError::NotFound)));
    assert!(matches!(random, Err(RestoreError::NotFound)));
    assert_eq!(rig.snapshot().await, before);
    rig.finish().await;
}
