//! AZ-3708 AC-1 (server name rules, IT-02-1) and AC-2 (case-insensitive uniqueness, AC-42), with the feed delta
//! of AC-7: one `seq` per success, none per failure.

use contracts::TreeServiceError;

use crate::mutation_support::Tree;

#[tokio::test]
async fn create_folder_refuses_each_forbidden_name_and_writes_nothing() {
    // Arrange
    let tree = Tree::new().await;
    let long = "a".repeat(256);
    let refused = [
        "a/b",
        "a\0b",
        ".",
        "..",
        "x\u{7}y",
        "x\u{202E}y",
        long.as_str(),
        "",
    ];
    let (nodes, seq) = (tree.nodes().await, tree.seq().await);

    // Act
    let mut outcomes = Vec::new();
    for name in refused {
        outcomes.push(
            tree.service
                .create_folder(tree.account, tree.root, name)
                .await,
        );
    }

    // Assert
    for (name, outcome) in refused.iter().zip(&outcomes) {
        assert!(
            matches!(outcome, Err(TreeServiceError::NameInvalid(reason)) if !reason.is_empty()),
            "{name:?}: {outcome:?}"
        );
    }
    assert_eq!(tree.nodes().await, nodes, "0 rows written");
    assert_eq!(tree.seq().await, seq, "no seq consumed");
    tree.finish().await;
}

#[tokio::test]
async fn create_folder_stores_each_permitted_name_with_its_exact_spelling() {
    // Arrange
    let tree = Tree::new().await;
    let long = "b".repeat(255);
    let accepted = [
        "CON",
        "a:b",
        "..a",
        "trailing.",
        ".hidden",
        "emoji-\u{1F600}",
        long.as_str(),
        "A.txt",
    ];
    let seq = tree.seq().await;

    // Act
    let mut views = Vec::new();
    for name in accepted {
        views.push(
            tree.service
                .create_folder(tree.account, tree.root, name)
                .await
                .expect("accepted name"),
        );
    }

    // Assert
    for (name, view) in accepted.iter().zip(&views) {
        let stored = tree
            .count(&format!(
                "SELECT count(*) FROM nodes WHERE id = '{}' AND name = '{}' AND parent_id = '{}'",
                view.id,
                name.replace('\'', "''"),
                tree.root
            ))
            .await;
        assert_eq!(stored, 1, "{name:?} stored verbatim");
        assert_eq!(view.name, *name);
    }
    assert_eq!(tree.seq().await, seq + 8, "one seq per success");
    tree.finish().await;
}

#[tokio::test]
async fn a_name_differing_only_in_case_conflicts() {
    // Arrange
    let tree = Tree::new().await;
    tree.service
        .create_folder(tree.account, tree.root, "Docs")
        .await
        .expect("create Docs");
    let (nodes, seq) = (tree.nodes().await, tree.seq().await);

    // Act
    let outcome = tree
        .service
        .create_folder(tree.account, tree.root, "docs")
        .await;

    // Assert
    assert!(
        matches!(outcome, Err(TreeServiceError::NameConflict)),
        "{outcome:?}"
    );
    assert_eq!(tree.nodes().await, nodes);
    assert_eq!(tree.seq().await, seq);
    tree.finish().await;
}

/// G-1 (2026-11-11): an NFC and an NFD spelling are one name, and the first spelling stays stored as sent.
#[tokio::test]
async fn an_nfd_spelling_conflicts_with_the_nfc_one() {
    // Arrange
    let tree = Tree::new().await;
    let nfc = "\u{E9}";
    let nfd = "e\u{301}";
    let first = tree
        .service
        .create_folder(tree.account, tree.root, nfc)
        .await
        .expect("create the NFC spelling");
    let seq = tree.seq().await;

    // Act
    let outcome = tree
        .service
        .create_folder(tree.account, tree.root, nfd)
        .await;

    // Assert
    assert!(
        matches!(outcome, Err(TreeServiceError::NameConflict)),
        "{outcome:?}"
    );
    assert_eq!(first.name, nfc);
    assert_eq!(tree.seq().await, seq);
    tree.finish().await;
}
