//! AC-2: a role or state change commits in one transaction with its credential revocation and its audit row,
//! and rolls back as a whole.

mod support;

use std::sync::atomic::Ordering;

use contracts::{AccountState, AdminError, Role};
use support::harness::{Harness, admin_actor};
use support::seed;
use uuid::Uuid;

struct Scene {
    harness: Harness,
    admin: Uuid,
    user: Uuid,
}

async fn scene() -> Scene {
    let harness = Harness::new().await;
    let (admin, user) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = harness.fixture().await;
    seed::admin(&mut conn, admin).await;
    seed::user_with_credentials(&mut conn, user, "Ulrika Utest", "ulrika@example.test").await;
    Scene {
        harness,
        admin,
        user,
    }
}

async fn block_admin_action_rows(harness: &Harness) {
    harness
        .db
        .admin_exec(
            "ALTER TABLE audit_log ADD CONSTRAINT refuse_admin_action CHECK (event <> 'admin_action')",
        )
        .await;
}

#[tokio::test]
async fn setting_the_role_commits_the_change_and_its_audit_row() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;

    // Act
    let result = harness
        .service
        .update_user(&admin_actor(admin), user, Some(Role::Admin), None)
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(result.is_ok());
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("admin".into(), "active".into())
    );
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 1);
    assert_eq!(seed::revoked_credentials(&mut conn, user).await, (0, 0));
    harness.finish().await;
}

#[tokio::test]
async fn disabling_revokes_both_sessions_and_the_device_token_with_the_change() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;
    let caller = admin_actor(admin);

    // Act
    let promoted = harness
        .service
        .update_user(&caller, user, Some(Role::Admin), None)
        .await;
    let disabled = harness
        .service
        .update_user(&caller, user, None, Some(AccountState::Disabled))
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(promoted.is_ok() && disabled.is_ok());
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("admin".into(), "disabled".into())
    );
    assert_eq!(seed::revoked_credentials(&mut conn, user).await, (2, 1));
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 2);
    harness.finish().await;
}

#[tokio::test]
async fn a_failed_audit_write_rolls_back_the_state_change_and_the_revocations() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;
    block_admin_action_rows(&harness).await;

    // Act
    let result = harness
        .service
        .update_user(
            &admin_actor(admin),
            user,
            None,
            Some(AccountState::Disabled),
        )
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(matches!(
        result,
        Err(sunnycloud_admin::AdminCallError::Failed(
            AdminError::Unexpected(_)
        ))
    ));
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("user".into(), "active".into())
    );
    assert_eq!(seed::revoked_credentials(&mut conn, user).await, (0, 0));
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 0);
    harness.finish().await;
}

#[tokio::test]
async fn a_failed_audit_write_rolls_back_the_role_change() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;
    block_admin_action_rows(&harness).await;

    // Act
    let result = harness
        .service
        .update_user(&admin_actor(admin), user, Some(Role::Admin), None)
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(result.is_err());
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("user".into(), "active".into())
    );
    harness.finish().await;
}

#[tokio::test]
async fn role_and_state_in_one_call_commit_together_with_two_audit_rows() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;

    // Act
    let result = harness
        .service
        .update_user(
            &admin_actor(admin),
            user,
            Some(Role::Admin),
            Some(AccountState::Disabled),
        )
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(result.is_ok());
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("admin".into(), "disabled".into())
    );
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 2);
    harness.finish().await;
}

#[tokio::test]
async fn a_failing_state_change_rolls_back_the_role_change_of_the_same_call() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;
    harness
        .accounts
        .fail_set_state
        .store(true, Ordering::SeqCst);

    // Act
    let result = harness
        .service
        .update_user(
            &admin_actor(admin),
            user,
            Some(Role::Admin),
            Some(AccountState::Disabled),
        )
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(result.is_err());
    assert_eq!(
        seed::role_and_state(&mut conn, user).await,
        ("user".into(), "active".into())
    );
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 0);
    harness.finish().await;
}

#[tokio::test]
async fn a_call_that_changes_nothing_writes_nothing() {
    // Arrange
    let Scene {
        harness,
        admin,
        user,
    } = scene().await;

    // Act
    let result = harness
        .service
        .update_user(&admin_actor(admin), user, None, None)
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    assert!(result.is_ok());
    assert_eq!(harness.accounts.mutations.load(Ordering::SeqCst), 0);
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 0);
    harness.finish().await;
}
