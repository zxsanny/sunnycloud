//! AZ-3927 AC-3 (IT-20-4; owner decision: no link sharing of vault files): over the real 02, a share of a file in
//! a vault and of the vault root folder is refused with `TargetNotShareable` (422 `target_not_shareable` at 11) and
//! writes no `share_links` row, while a file beside the vault is shared. 06 reads the membership from 02's
//! `resolve_live_file`; it never calls 20.

use contracts::{ShareError, ShareService};
use uuid::Uuid;

use crate::content_support::Owner;
use crate::harness::Harness;

async fn links_of(owner: &mut Owner, node: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM share_links WHERE node_id = $1")
        .bind(node)
        .fetch_one(owner.probe())
        .await
        .expect("count links")
}

#[tokio::test]
async fn a_vault_file_and_the_vault_root_are_not_shareable_and_a_file_beside_the_vault_is() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let vault_root = owner.folder(owner.root, "Vault").await;
    owner.make_vault(&harness, vault_root).await;
    let inner = owner.folder(vault_root, "inner").await;
    let vault_file = owner.new_file(&harness, inner, "secret.jpg").await;
    let beside = owner.new_file(&harness, owner.root, "plain.jpg").await;
    let shares = harness.shares();

    // Act
    let of_vault_file = shares
        .create(owner.account, vault_file.node, None, None)
        .await;
    let of_vault_root = shares.create(owner.account, vault_root, None, None).await;
    let of_beside = shares.create(owner.account, beside.node, None, None).await;

    // Assert
    assert!(
        matches!(of_vault_file, Err(ShareError::TargetNotShareable)),
        "{of_vault_file:?}"
    );
    assert!(
        matches!(of_vault_root, Err(ShareError::TargetNotShareable)),
        "{of_vault_root:?}"
    );
    of_beside.expect("a file beside the vault is shared");
    assert_eq!(links_of(&mut owner, vault_file.node).await, 0);
    assert_eq!(links_of(&mut owner, vault_root).await, 0);
    assert_eq!(links_of(&mut owner, beside.node).await, 1);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}
