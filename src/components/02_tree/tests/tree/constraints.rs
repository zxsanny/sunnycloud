//! AC-1: the CHECK, unique and composite foreign key constraints of `nodes` and `revisions`, each probed with a
//! row that breaks only that rule, as the schema owner (row-level security does not apply to it).

use sqlx::Connection;
use uuid::Uuid;

use crate::support::{
    Fixture, RevisionRow, account, account_with_root, blob, node, node_row, refusal, revision,
};

const CHECK_VIOLATION: &str = "23514";
const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_VIOLATION: &str = "23503";

fn refused(sqlstate: &str, constraint: &str) -> (String, String) {
    (sqlstate.to_owned(), constraint.to_owned())
}

struct Accounts {
    fixture: Fixture,
    a: Uuid,
    root: Uuid,
    b_root: Uuid,
}

async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root = account_with_root(&mut conn, a).await;
    let b_root = account_with_root(&mut conn, b).await;
    conn.close().await.expect("close fixture connection");
    Accounts {
        fixture,
        a,
        root,
        b_root,
    }
}

#[tokio::test]
async fn node_rows_that_break_one_rule_are_refused_by_that_rule() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let (a, root) = (accounts.a, accounts.root);
    let key_1025 = "k".repeat(1025);
    let name_256 = "n".repeat(256);
    let two_byte_name = "é".repeat(128);
    let cases: Vec<(&str, Option<Uuid>, &str, String, String)> = vec![
        ("bad kind", Some(root), "link", "x".into(), "x".into()),
        (
            "file without parent",
            None,
            "file",
            String::new(),
            String::new(),
        ),
        (
            "empty name under a parent",
            Some(root),
            "folder",
            String::new(),
            String::new(),
        ),
        ("dot", Some(root), "folder", ".".into(), ".".into()),
        ("dot dot", Some(root), "folder", "..".into(), "..".into()),
        ("slash", Some(root), "folder", "a/b".into(), "a/b".into()),
        ("root with a name", None, "folder", "x".into(), "x".into()),
        (
            "name of 256 bytes",
            Some(root),
            "folder",
            name_256,
            "n".into(),
        ),
        (
            "name of 256 bytes in two-byte characters",
            Some(root),
            "folder",
            two_byte_name,
            "e".into(),
        ),
        (
            "name key of 1025 bytes",
            Some(root),
            "folder",
            "k".into(),
            key_1025,
        ),
    ];

    // Act
    let mut outcomes = Vec::new();
    for (label, parent, kind, name, key) in cases {
        let result = node_row(&mut conn, Uuid::new_v4(), a, parent, kind, &name, &key).await;
        outcomes.push((label, refusal(result)));
    }

    // Assert
    assert_eq!(
        outcomes,
        vec![
            ("bad kind", refused(CHECK_VIOLATION, "nodes_kind_check")),
            (
                "file without parent",
                refused(CHECK_VIOLATION, "nodes_file_parent_check")
            ),
            (
                "empty name under a parent",
                refused(CHECK_VIOLATION, "nodes_name_check")
            ),
            ("dot", refused(CHECK_VIOLATION, "nodes_name_check")),
            ("dot dot", refused(CHECK_VIOLATION, "nodes_name_check")),
            ("slash", refused(CHECK_VIOLATION, "nodes_name_check")),
            (
                "root with a name",
                refused(CHECK_VIOLATION, "nodes_name_check")
            ),
            (
                "name of 256 bytes",
                refused(CHECK_VIOLATION, "nodes_name_size_check")
            ),
            (
                "name of 256 bytes in two-byte characters",
                refused(CHECK_VIOLATION, "nodes_name_size_check")
            ),
            (
                "name key of 1025 bytes",
                refused(CHECK_VIOLATION, "nodes_name_key_size_check")
            ),
        ]
    );
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn names_at_the_limits_are_accepted() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let (a, root) = (accounts.a, accounts.root);

    // Act
    let name_255 = node_row(
        &mut conn,
        Uuid::new_v4(),
        a,
        Some(root),
        "folder",
        &"n".repeat(255),
        "n",
    )
    .await;
    let key_1024 = node_row(
        &mut conn,
        Uuid::new_v4(),
        a,
        Some(root),
        "folder",
        "k",
        &"k".repeat(1024),
    )
    .await;
    let dotted = node_row(
        &mut conn,
        Uuid::new_v4(),
        a,
        Some(root),
        "file",
        "...",
        "...",
    )
    .await;

    // Assert
    name_255.expect("255 bytes");
    key_1024.expect("1024 bytes");
    dotted.expect("three dots is an ordinary name");
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn folder_with_a_current_revision_and_a_second_root_are_refused() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let (a, root) = (accounts.a, accounts.root);

    // Act
    let folder_with_revision = sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, current_revision_id, created_at, updated_at) \
         VALUES ($1, $2, $3, 'folder', 'f', 'f', 1, $4, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(Uuid::new_v4())
    .bind(a)
    .bind(root)
    .bind(Uuid::new_v4())
    .execute(&mut conn)
    .await;
    let second_root = node(&mut conn, a, None, "folder", "").await;

    // Assert
    assert_eq!(
        refusal(folder_with_revision),
        refused(CHECK_VIOLATION, "nodes_folder_revision_check")
    );
    assert_eq!(
        refusal(second_root),
        refused(UNIQUE_VIOLATION, "nodes_root_key")
    );
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn parent_of_another_account_or_a_missing_parent_is_refused() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;

    // Act
    let foreign_parent = node(&mut conn, accounts.a, Some(accounts.b_root), "folder", "x").await;
    let missing_parent = node(&mut conn, accounts.a, Some(Uuid::new_v4()), "folder", "y").await;

    // Assert
    for result in [foreign_parent, missing_parent] {
        assert_eq!(
            refusal(result),
            refused(FOREIGN_KEY_VIOLATION, "nodes_parent_id_account_id_fkey")
        );
    }
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn parent_with_children_cannot_be_deleted() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    node(
        &mut conn,
        accounts.a,
        Some(accounts.root),
        "folder",
        "child",
    )
    .await
    .expect("child");

    // Act
    let delete = sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(accounts.root)
        .execute(&mut conn)
        .await;

    // Assert
    assert_eq!(
        refusal(delete),
        refused("23503", "nodes_parent_id_account_id_fkey")
    );
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn revision_rows_that_break_one_rule_are_refused_by_that_rule() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let a = accounts.a;
    let file = node(&mut conn, a, Some(accounts.root), "file", "f.txt")
        .await
        .expect("file");
    let big_meta = format!(r#"{{"device":"{}"}}"#, "d".repeat(4100));
    let mut blobs = Vec::new();
    for _ in 0..6 {
        let id = Uuid::new_v4();
        blob(&mut conn, id, a).await;
        blobs.push(id);
    }
    let rows = [
        RevisionRow {
            size: -1,
            ..RevisionRow::new(a, file, blobs[0])
        },
        RevisionRow {
            content_hash: &[1; 31],
            ..RevisionRow::new(a, file, blobs[1])
        },
        RevisionRow {
            blocklist: &[2; 33],
            ..RevisionRow::new(a, file, blobs[2])
        },
        RevisionRow {
            client_meta: Some(&big_meta),
            ..RevisionRow::new(a, file, blobs[3])
        },
    ];

    // Act
    let mut outcomes = Vec::new();
    for row in &rows {
        outcomes.push(refusal(revision(&mut conn, row).await));
    }

    // Assert
    assert_eq!(
        outcomes,
        vec![
            refused(CHECK_VIOLATION, "revisions_size_check"),
            refused(CHECK_VIOLATION, "revisions_content_hash_check"),
            refused(CHECK_VIOLATION, "revisions_blocklist_check"),
            refused(CHECK_VIOLATION, "revisions_client_meta_check"),
        ]
    );
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn revision_values_at_the_limits_are_accepted() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let a = accounts.a;
    let file = node(&mut conn, a, Some(accounts.root), "file", "f.txt")
        .await
        .expect("file");
    // jsonb prints a space after the colon, which the size check counts.
    let meta_4096 = format!(r#"{{"device":"{}"}}"#, "d".repeat(4096 - 14));
    let mut rows = Vec::new();
    for _ in 0..3 {
        let id = Uuid::new_v4();
        blob(&mut conn, id, a).await;
        rows.push(id);
    }

    // Act
    let empty_blocklist = revision(
        &mut conn,
        &RevisionRow {
            size: 0,
            blocklist: &[],
            ..RevisionRow::new(a, file, rows[0])
        },
    )
    .await;
    let two_blocks = revision(
        &mut conn,
        &RevisionRow {
            blocklist: &[3; 64],
            ..RevisionRow::new(a, file, rows[1])
        },
    )
    .await;
    let full_meta = revision(
        &mut conn,
        &RevisionRow {
            client_meta: Some(&meta_4096),
            ..RevisionRow::new(a, file, rows[2])
        },
    )
    .await;

    // Assert
    empty_blocklist.expect("zero blocks");
    two_blocks.expect("two blocks");
    full_meta.expect("metadata of 4096 bytes");
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn one_blob_backs_one_revision_and_both_parents_must_belong_to_the_account() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let (a, b_account) = (accounts.a, Uuid::new_v4());
    account(&mut conn, b_account).await;
    let file = node(&mut conn, a, Some(accounts.root), "file", "f.txt")
        .await
        .expect("file");
    let shared_blob = Uuid::new_v4();
    blob(&mut conn, shared_blob, a).await;
    revision(&mut conn, &RevisionRow::new(a, file, shared_blob))
        .await
        .expect("first revision");
    let foreign_blob = Uuid::new_v4();
    blob(&mut conn, foreign_blob, b_account).await;
    let own_blob = Uuid::new_v4();
    blob(&mut conn, own_blob, a).await;

    // Act
    let second_on_blob = revision(&mut conn, &RevisionRow::new(a, file, shared_blob)).await;
    let blob_of_another_account =
        revision(&mut conn, &RevisionRow::new(a, file, foreign_blob)).await;
    let node_of_another_account =
        revision(&mut conn, &RevisionRow::new(a, accounts.b_root, own_blob)).await;

    // Assert
    assert_eq!(
        refusal(second_on_blob),
        refused(UNIQUE_VIOLATION, "revisions_blob_id_key")
    );
    assert_eq!(
        refusal(blob_of_another_account),
        refused(FOREIGN_KEY_VIOLATION, "revisions_blob_id_account_id_fkey")
    );
    assert_eq!(
        refusal(node_of_another_account),
        refused(FOREIGN_KEY_VIOLATION, "revisions_node_id_account_id_fkey")
    );
    conn.close().await.expect("close fixture connection");
    accounts.fixture.finish().await;
}
