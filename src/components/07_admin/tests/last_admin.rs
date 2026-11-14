//! AZ-3818 AC-1 (IT-07-3; FT-N-46, FT-P-65): the only active admin can be neither demoted, nor disabled, nor
//! deleted, by itself or by the operator; a disabled admin does not count. After a promotion the demotion
//! passes. Real 01 and 07a on a real PostgreSQL.

mod support;

use contracts::{
    AccountDeletionService, AccountState, Actor, AdminError, AdminService, DeletionError,
    PrincipalKind, Role,
};
use support::account_data::deletion_requested_at;
use support::harness::{START, admin_actor, principal};
use support::seed::{
    account, admin, audit_rows, quota, revoked_credentials, role_and_state, session,
    user_with_credentials,
};
use support::world::World;
use uuid::Uuid;

fn last_admin(result: Result<(), AdminError>) -> bool {
    matches!(result, Err(AdminError::LastAdmin))
}

#[tokio::test]
async fn the_only_active_admin_cannot_be_removed_on_any_path_until_another_admin_exists() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let (a, disabled_admin) = (Uuid::new_v4(), Uuid::new_v4());
    let (first_user, second_user) = (Uuid::new_v4(), Uuid::new_v4());
    admin(&mut conn, a).await;
    session(&mut conn, a).await;
    account(
        &mut conn,
        disabled_admin,
        "Disabled Admin",
        "admin",
        "disabled",
        START.as_unix_micros(),
    )
    .await;
    quota(&mut conn, disabled_admin, None, 0, 0).await;
    user_with_credentials(&mut conn, first_user, "First User", "first@example.test").await;
    user_with_credentials(&mut conn, second_user, "Second User", "second@example.test").await;
    let own = principal(a, PrincipalKind::Session, Role::Admin);
    let callers = [admin_actor(a), Actor::Operator];

    // Act
    let mut refusals = Vec::new();
    for caller in &callers {
        let demoted = world
            .admin
            .update_user(caller, a, Some(Role::User), None)
            .await;
        let disabled = world
            .admin
            .update_user(caller, a, None, Some(AccountState::Disabled))
            .await;
        refusals.extend([last_admin(demoted), last_admin(disabled)]);
    }
    let own_deletion = world.deletion.request(&own, &a.to_string()).await;
    let operator_deletion = world.deletion.request_as_operator(a).await;
    refusals.extend([
        matches!(own_deletion, Err(DeletionError::LastAdmin)),
        matches!(operator_deletion, Err(DeletionError::LastAdmin)),
    ]);
    let after_refusals = (
        role_and_state(&mut conn, a).await,
        revoked_credentials(&mut conn, a).await,
        deletion_requested_at(&mut conn, a).await,
        audit_rows(&mut conn).await.len(),
    );
    let promoted = world
        .admin
        .update_user(&callers[0], first_user, Some(Role::Admin), None)
        .await;
    let demoted_after_promotion = world
        .admin
        .update_user(&callers[0], a, Some(Role::User), None)
        .await;

    // Assert
    assert_eq!(refusals, [true; 6]);
    assert_eq!(
        after_refusals,
        (("admin".into(), "active".into()), (0, 0), None, 0)
    );
    assert!(promoted.is_ok(), "{promoted:?}");
    assert!(
        demoted_after_promotion.is_ok(),
        "{demoted_after_promotion:?}"
    );
    assert_eq!(
        role_and_state(&mut conn, a).await,
        ("user".into(), "active".into())
    );
    assert_eq!(
        role_and_state(&mut conn, first_user).await,
        ("admin".into(), "active".into())
    );
    drop(conn);
    world.finish().await;
}
