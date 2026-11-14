//! AZ-3927 AC-4 (AC-60, FT-N-15 step 1b, NFT-SEC-40 step 1b) and AC-5 (IT-20-4 gaps): the move rules of E-30/c01
//! (`20_vault_service` section 10, item 5) through the real 02 `update_node`, one test per rule. A move between
//! folders of one vault is allowed and changes nothing of the file but its parent; a move into, out of or between
//! vaults is `VaultBoundary` and changes nothing; a plaintext file with an active share can never enter a vault; a
//! vault root moves with its vault between folders outside every vault.

use contracts::{TreeServiceError, VaultError};
use uuid::Uuid;

use crate::real_ports::jpeg;
use crate::real_support::Real;

const WRAPPED_KEY_1: &str = r#"{"device": "laptop", "vault_key": "d3JhcHBlZC0x"}"#;
const WRAPPED_KEY_2: &str = r#"{"device": "laptop", "vault_key": "d3JhcHBlZC0y"}"#;

/// A vault rooted at `Vault` under the account root, with the folders `a` and `b` in it.
async fn vault_with_two_folders(real: &Real) -> (Uuid, Uuid, Uuid, Uuid) {
    let root = real.make_folder(real.root, "Vault").await;
    let vault = real.create_vault(root).await;
    let a = real.make_folder(root, "a").await;
    let b = real.make_folder(root, "b").await;
    (vault, root, a, b)
}

#[tokio::test]
async fn a_move_inside_one_vault_keeps_the_node_its_revisions_their_client_meta_and_blobs() {
    // Arrange
    let real = Real::new().await;
    let (vault, _, a, b) = vault_with_two_folders(&real).await;
    let first = real
        .commit_to(a, "secret.jpg", &jpeg("one"), None, Some(WRAPPED_KEY_1))
        .await;
    let second = real
        .commit_to(
            a,
            "secret.jpg",
            &jpeg("two"),
            Some(first.revision_id),
            Some(WRAPPED_KEY_2),
        )
        .await;
    let before = real.revisions_of(first.node_id).await;

    // Act
    let moved = real
        .service
        .update_node(real.account, first.node_id, None, Some(b))
        .await;
    let after = real.revisions_of(first.node_id).await;
    let later = real
        .commit_to(
            b,
            "secret.jpg",
            &jpeg("three"),
            Some(second.revision_id),
            None,
        )
        .await;

    // Assert
    let moved = moved.expect("a move inside the vault");
    assert_eq!((moved.id, moved.parent_id), (first.node_id, Some(b)));
    assert_eq!(before.len(), 2);
    assert_eq!(after, before);
    assert_eq!(later.node_id, first.node_id);
    assert!(real.seen_vault(later.revision_id));
    assert_eq!(real.jobs_for(later.revision_id).await, 0);
    assert_eq!(real.vault_of(first.node_id).await, Some(vault));
    real.finish().await;
}

#[tokio::test]
async fn a_move_into_a_vault_is_vault_boundary_and_changes_nothing() {
    // Arrange
    let real = Real::new().await;
    let (_, vault_root, a, _) = vault_with_two_folders(&real).await;
    let plain = real
        .commit_to(real.root, "plain.jpg", &jpeg("plain"), None, None)
        .await;
    let folder = real.make_folder(real.root, "folder").await;
    let rows = real.rows().await;

    // Act
    let file_in = real
        .service
        .update_node(real.account, plain.node_id, None, Some(a))
        .await;
    let folder_in = real
        .service
        .update_node(real.account, folder, Some("renamed"), Some(vault_root))
        .await;

    // Assert
    assert!(
        matches!(file_in, Err(TreeServiceError::VaultBoundary)),
        "{file_in:?}"
    );
    assert!(
        matches!(folder_in, Err(TreeServiceError::VaultBoundary)),
        "{folder_in:?}"
    );
    assert_eq!(real.rows().await, rows);
    real.finish().await;
}

#[tokio::test]
async fn a_move_out_of_a_vault_is_vault_boundary_and_changes_nothing() {
    // Arrange
    let real = Real::new().await;
    let (vault, _, a, _) = vault_with_two_folders(&real).await;
    let secret = real
        .commit_to(a, "secret.jpg", &jpeg("secret"), None, Some(WRAPPED_KEY_1))
        .await;
    let rows = real.rows().await;

    // Act
    let file_out = real
        .service
        .update_node(real.account, secret.node_id, None, Some(real.root))
        .await;
    let folder_out = real
        .service
        .update_node(real.account, a, None, Some(real.root))
        .await;

    // Assert
    assert!(
        matches!(file_out, Err(TreeServiceError::VaultBoundary)),
        "{file_out:?}"
    );
    assert!(
        matches!(folder_out, Err(TreeServiceError::VaultBoundary)),
        "{folder_out:?}"
    );
    assert_eq!(real.rows().await, rows);
    assert_eq!(real.vault_of(secret.node_id).await, Some(vault));
    real.finish().await;
}

#[tokio::test]
async fn a_plaintext_file_with_an_active_share_can_neither_seed_a_vault_nor_move_into_one() {
    // Arrange
    let real = Real::new().await;
    let holder = real.make_folder(real.root, "holder").await;
    let in_holder = real
        .commit_to(holder, "shared.jpg", &jpeg("in holder"), None, None)
        .await;
    let shared = real
        .commit_to(real.root, "shared.jpg", &jpeg("shared"), None, None)
        .await;
    for node in [in_holder.node_id, shared.node_id] {
        share(&real, node).await;
    }

    // Act
    let seeded = real.try_create_vault(holder).await;
    let (vault, vault_root, _, _) = vault_with_two_folders(&real).await;
    let moved = real
        .service
        .update_node(real.account, shared.node_id, None, Some(vault_root))
        .await;

    // Assert
    assert!(matches!(seeded, Err(VaultError::VaultExists)), "{seeded:?}");
    assert!(
        matches!(moved, Err(TreeServiceError::VaultBoundary)),
        "{moved:?}"
    );
    assert_eq!(real.parent_of(shared.node_id).await, Some(real.root));
    assert_eq!(
        [
            real.vault_of(holder).await,
            real.vault_of(in_holder.node_id).await,
            real.vault_of(shared.node_id).await,
            real.vault_of(vault_root).await,
        ],
        [None, None, None, Some(vault)]
    );
    let active = format!(
        "SELECT count(*) FROM share_links WHERE account_id = '{}' AND revoked_at IS NULL",
        real.account
    );
    assert_eq!(real.count(&active).await, 2);
    real.finish().await;
}

#[tokio::test]
async fn a_vault_root_moves_with_its_vault_between_folders_outside_every_vault() {
    // Arrange
    let real = Real::new().await;
    let (vault, vault_root, a, _) = vault_with_two_folders(&real).await;
    let secret = real
        .commit_to(a, "secret.jpg", &jpeg("secret"), None, Some(WRAPPED_KEY_1))
        .await;
    let shelf = real.make_folder(real.root, "shelf").await;

    // Act
    let moved = real
        .service
        .update_node(real.account, vault_root, None, Some(shelf))
        .await;

    // Assert
    let moved = moved.expect("a vault root moves outside every vault");
    assert_eq!(moved.parent_id, Some(shelf));
    assert!(moved.vault);
    assert_eq!(
        [
            real.vault_of(vault_root).await,
            real.vault_of(a).await,
            real.vault_of(secret.node_id).await,
            real.vault_of(shelf).await,
        ],
        [Some(vault), Some(vault), Some(vault), None]
    );
    real.finish().await;
}

/// An active link on `node`, written past 06 (02 has no 06 port): the row 06's `create` writes.
async fn share(real: &Real, node: Uuid) {
    real.fixture
        .db
        .admin_exec(&format!(
            "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
             VALUES ('{}', '{}', '{node}', sha256('{node}'::text::bytea), \
             'epoch'::timestamptz + interval '100 years', 'epoch'::timestamptz)",
            Uuid::new_v4(),
            real.account
        ))
        .await;
}
