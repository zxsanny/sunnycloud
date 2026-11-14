//! AZ-3819 AC-1 (IT-07-7): `view` and `quota_view` return the caller's own account only, from the real 01, 02
//! and 07a on a real PostgreSQL.

mod support;

use contracts::{AccountService, IdentityView, PrincipalKind, Role};
use support::account_data::{linked_identity, live_file, root};
use support::harness::START;
use support::harness::principal;
use support::seed::{account, quota};
use support::world::World;
use uuid::Uuid;

const SECOND_MICROS: i64 = 1_000_000;

fn identity(provider: &str, email: Option<&str>) -> IdentityView {
    IdentityView {
        provider: provider.to_owned(),
        email: email.map(str::to_owned),
    }
}

#[tokio::test]
async fn each_caller_sees_its_own_account_identities_root_and_quota() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let at = START.as_unix_micros();
    account(&mut conn, a, "Ada", "user", "active", at).await;
    account(&mut conn, b, "Bob", "admin", "active", at).await;
    quota(&mut conn, a, Some(1 << 30), 300, 100).await;
    quota(&mut conn, b, None, 7, 0).await;
    linked_identity(&mut conn, a, "google", Some("ada@example.test"), at).await;
    linked_identity(&mut conn, a, "github", None, at + SECOND_MICROS).await;
    linked_identity(&mut conn, b, "github", Some("bob@example.test"), at).await;
    let root_a = root(&mut conn, a).await;
    let root_b = root(&mut conn, b).await;
    live_file(&mut conn, a, root_a).await;
    let caller_a = principal(a, PrincipalKind::Session, Role::User);
    let caller_b = principal(b, PrincipalKind::Device, Role::Admin);

    // Act
    let view_a = world.accounts.view(&caller_a).await.expect("view of A");
    let quota_a = world
        .accounts
        .quota_view(&caller_a)
        .await
        .expect("quota of A");
    let view_b = world.accounts.view(&caller_b).await.expect("view of B");
    let quota_b = world
        .accounts
        .quota_view(&caller_b)
        .await
        .expect("quota of B");

    // Assert
    let tree_root_a = world.tree.root_node(a).await.expect("root of A").id;
    let ledger_a = world.quota.view(a).await.expect("07a view of A");
    let ledger_b = world.quota.view(b).await.expect("07a view of B");
    assert_eq!(
        (view_a.id, view_a.display_name.as_str(), view_a.role),
        (a, "Ada", Role::User)
    );
    assert_eq!(
        view_a.identities,
        [
            identity("google", Some("ada@example.test")),
            identity("github", None)
        ]
    );
    assert_eq!((view_a.root_node_id, tree_root_a), (root_a, root_a));
    assert_eq!((quota_a, view_a.quota), (ledger_a, ledger_a));
    assert_eq!(
        (ledger_a.limit_bytes, ledger_a.used, ledger_a.reserved),
        (Some(1 << 30), 300, 100)
    );
    assert_eq!(
        (view_b.id, view_b.display_name.as_str(), view_b.role),
        (b, "Bob", Role::Admin)
    );
    assert_eq!(
        view_b.identities,
        [identity("github", Some("bob@example.test"))]
    );
    assert_eq!(view_b.root_node_id, root_b);
    assert_eq!((quota_b, view_b.quota), (ledger_b, ledger_b));
    assert_ne!(quota_b, quota_a);
    drop(conn);
    world.finish().await;
}
