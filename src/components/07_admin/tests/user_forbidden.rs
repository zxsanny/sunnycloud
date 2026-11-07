//! AC-6: only an admin session or the operator may administer users.

mod support;

use contracts::{AccountState, Actor, Page, PrincipalKind, Role};
use sunnycloud_admin::AdminCallError;
use support::harness::{Harness, admin_actor, principal};
use support::seed;
use uuid::Uuid;

async fn scene() -> (Harness, Uuid) {
    let harness = Harness::new().await;
    let user = Uuid::new_v4();
    seed::user_with_credentials(
        &mut harness.fixture().await,
        user,
        "Ulrika Utest",
        "ulrika@example.test",
    )
    .await;
    (harness, user)
}

#[tokio::test]
async fn a_user_session_and_an_admin_device_token_are_forbidden_and_nothing_changes() {
    // Arrange
    let (harness, user) = scene().await;
    let account = Uuid::new_v4();
    let callers = [
        Actor::Admin(principal(account, PrincipalKind::Session, Role::User)),
        Actor::Admin(principal(account, PrincipalKind::Device, Role::Admin)),
    ];

    for caller in callers {
        // Act
        let list = harness
            .service
            .list_users(
                &caller,
                Page {
                    offset: 0,
                    limit: 20,
                },
            )
            .await;
        let update = harness
            .service
            .update_user(
                &caller,
                user,
                Some(Role::Admin),
                Some(AccountState::Disabled),
            )
            .await;
        let quota = harness
            .service
            .set_quota(&caller, user, Some(1 << 30))
            .await;

        // Assert
        assert!(matches!(list, Err(AdminCallError::Forbidden)), "{list:?}");
        assert!(
            matches!(update, Err(AdminCallError::Forbidden)),
            "{update:?}"
        );
        assert!(matches!(quota, Err(AdminCallError::Forbidden)), "{quota:?}");
    }
    let mut conn = harness.fixture().await;
    assert_eq!(harness.port_calls(), [0; 5]);
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("user".into(), "active".into())
    );
    assert_eq!(seed::limit_of(&mut conn, user).await, None);
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 0);
    harness.finish().await;
}

#[tokio::test]
async fn the_operator_and_an_admin_session_are_accepted() {
    // Arrange
    let (harness, user) = scene().await;
    let admin = Uuid::new_v4();
    seed::admin(&mut harness.fixture().await, admin).await;

    for caller in [Actor::Operator, admin_actor(admin)] {
        // Act
        let list = harness
            .service
            .list_users(
                &caller,
                Page {
                    offset: 0,
                    limit: 20,
                },
            )
            .await;
        let update = harness
            .service
            .update_user(&caller, user, Some(Role::Admin), None)
            .await;
        let quota = harness
            .service
            .set_quota(&caller, user, Some(1 << 30))
            .await;

        // Assert
        assert_eq!(list.expect("list").len(), 2);
        assert!(update.is_ok());
        assert!(quota.is_ok());
    }
    harness.finish().await;
}
