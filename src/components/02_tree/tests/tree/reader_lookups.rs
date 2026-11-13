//! AZ-3707 AC-4 to AC-8 (IT-02-17, ST-02-1 subset): file, folder and gone are distinct; a file under a deleted
//! folder is gone before the cascade; a node id resolves across rename and move; the live lookup by folded name;
//! another account's ids answer like random ids.

use contracts::{LiveFile, LiveFileLookup, NodeKind, NodeRef, NodeSelector, NodeState};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::SIZE;
use crate::reader_support::{publish_file, reader};

#[tokio::test]
async fn a_live_file_and_folder_are_found_and_tombstoned_purged_and_foreign_files_are_gone() {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let (live, revision, blob) = publish_file(&tree, account, root, "live.txt").await;
    let folder = tree
        .service
        .create_folder(account, root, "folder")
        .await
        .expect("folder")
        .id;
    let (tombstoned, _, _) = publish_file(&tree, account, root, "deleted.txt").await;
    tree.service
        .delete_node(account, tombstoned, None)
        .await
        .expect("delete");
    let (purged, _, _) = publish_file(&tree, account, root, "purged.txt").await;
    tree.exec(&format!("DELETE FROM revisions WHERE node_id = '{purged}'"))
        .await;
    tree.exec(&format!("DELETE FROM nodes WHERE id = '{purged}'"))
        .await;
    let (other, other_root) = tree.add_account().await;
    let (foreign, _, _) = publish_file(&tree, other, other_root, "theirs.txt").await;
    let reader = reader(&tree, 1000);

    // Act
    let mut results = Vec::new();
    for node in [live, folder, tombstoned, purged, foreign] {
        results.push(
            reader
                .resolve_live_file(account, node)
                .await
                .expect("resolve"),
        );
    }

    // Assert
    let LiveFileLookup::Found(LiveFile {
        kind: NodeKind::File,
        revision: Some(current),
        blob_ref: Some(blob_ref),
    }) = &results[0]
    else {
        panic!("live file: {:?}", results[0]);
    };
    assert_eq!(
        (current.id, current.node_id, current.size, *blob_ref),
        (revision, live, SIZE, blob)
    );
    assert_eq!(
        results[1..],
        [
            LiveFileLookup::Found(LiveFile {
                kind: NodeKind::Folder,
                revision: None,
                blob_ref: None,
            }),
            LiveFileLookup::Gone,
            LiveFileLookup::Gone,
            LiveFileLookup::Gone,
        ]
    );
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn a_file_under_a_deleted_folder_is_gone_before_the_cascade_marks_it() {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let folder = tree
        .service
        .create_folder(account, root, "A")
        .await
        .expect("folder")
        .id;
    let (file, _, _) = publish_file(&tree, account, folder, "inside.txt").await;
    let reader = reader(&tree, 1000);
    let before = reader
        .resolve_live_file(account, file)
        .await
        .expect("resolve");

    // Act
    tree.service
        .delete_node(account, folder, None)
        .await
        .expect("delete folder");
    let after = reader
        .resolve_live_file(account, file)
        .await
        .expect("resolve");
    let node = reader
        .get_node(account, NodeSelector::Id(file))
        .await
        .expect("node");

    // Assert
    assert!(matches!(before, LiveFileLookup::Found(_)));
    assert_eq!(after, LiveFileLookup::Gone);
    assert_eq!(node.state, NodeState::Tombstone);
    let unmarked = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{file}' AND deleted_at IS NULL"
        ))
        .await;
    assert_eq!(unmarked, 1, "no cascade has marked the file");
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn a_file_id_resolves_after_rename_and_move_and_is_gone_on_the_first_call_after_delete() {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let target = tree
        .service
        .create_folder(account, root, "target")
        .await
        .expect("folder")
        .id;
    let (file, revision, _) = publish_file(&tree, account, root, "shared.txt").await;
    let reader = reader(&tree, 1000);
    let current = async |reader: &sunnycloud_tree::PgTreeReader| match reader
        .resolve_live_file(account, file)
        .await
        .expect("resolve")
    {
        LiveFileLookup::Found(found) => found.revision.map(|revision| revision.id),
        LiveFileLookup::Gone => None,
    };

    // Act
    tree.service
        .update_node(account, file, Some("renamed.txt"), None)
        .await
        .expect("rename");
    let after_rename = current(&reader).await;
    tree.service
        .update_node(account, file, None, Some(target))
        .await
        .expect("move");
    let after_move = current(&reader).await;
    tree.service
        .delete_node(account, file, None)
        .await
        .expect("delete");
    let after_delete = reader
        .resolve_live_file(account, file)
        .await
        .expect("resolve");

    // Assert
    assert_eq!(after_rename, Some(revision));
    assert_eq!(after_move, Some(revision));
    assert_eq!(after_delete, LiveFileLookup::Gone);
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn find_live_node_matches_the_folded_name_and_never_a_tombstone() {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let (upper, _, _) = publish_file(&tree, account, root, "A.txt").await;
    let (deleted, _, _) = publish_file(&tree, account, root, "b.txt").await;
    tree.service
        .delete_node(account, deleted, None)
        .await
        .expect("delete");
    let reader = reader(&tree, 1000);

    // Act
    let folded = reader
        .find_live_node(account, root, "a.txt")
        .await
        .expect("lookup");
    let tombstone = reader
        .find_live_node(account, root, "b.txt")
        .await
        .expect("lookup");

    // Assert
    assert_eq!(
        folded,
        Some(NodeRef {
            id: upper,
            kind: NodeKind::File,
        })
    );
    assert_eq!(tombstone, None);
    drop(reader);
    tree.finish().await;
}

/// What account `account` gets from every read for `folder`, `file` and `name`, as text.
async fn answers(
    reader: &sunnycloud_tree::PgTreeReader,
    account: Uuid,
    folder: Uuid,
    file: Uuid,
) -> [String; 4] {
    [
        format!(
            "{:?}",
            reader.get_node(account, NodeSelector::Id(file)).await
        ),
        format!("{:?}", reader.list_children(account, folder, None, 0).await),
        format!(
            "{:?}",
            reader.find_live_node(account, folder, "theirs.txt").await
        ),
        format!("{:?}", reader.resolve_live_file(account, file).await),
    ]
}

#[tokio::test]
async fn another_accounts_ids_answer_exactly_like_random_ids() {
    // Arrange
    let tree = Tree::new().await;
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "private")
        .await
        .expect("folder")
        .id;
    let (file, _, _) = publish_file(&tree, tree.account, folder, "theirs.txt").await;
    let (other, other_root) = tree.add_account().await;
    let reader = reader(&tree, 1000);

    // Act
    let foreign = answers(&reader, other, folder, file).await;
    let random = answers(&reader, other, Uuid::new_v4(), Uuid::new_v4()).await;
    let own_root = reader.root_node(other).await.expect("root");
    let root_view = reader
        .get_node(other, NodeSelector::Root)
        .await
        .expect("root view");

    // Assert
    assert_eq!(foreign, random);
    assert_eq!(
        foreign,
        [
            "Err(NotFound)".to_owned(),
            "Err(NotFound)".to_owned(),
            "Ok(None)".to_owned(),
            "Ok(Gone)".to_owned(),
        ]
    );
    assert_eq!(
        own_root,
        NodeRef {
            id: other_root,
            kind: NodeKind::Folder,
        }
    );
    assert_eq!((root_view.id, root_view.parent_id), (other_root, None));
    drop(reader);
    tree.finish().await;
}
