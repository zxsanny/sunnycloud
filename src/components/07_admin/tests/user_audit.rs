//! AC-4: the four changes of AC-2 and AC-3 leave four `admin_action` rows with ids only.

mod support;

use contracts::{AccountState, Role};
use support::harness::{Harness, admin_actor, principal};
use support::seed;
use uuid::Uuid;

const GIB: i64 = 1 << 30;
const NAME: &str = "Ulrika Utest";
const EMAIL: &str = "ulrika.utest@example.test";

#[tokio::test]
async fn four_changes_write_four_rows_with_actor_subject_and_action_and_no_name_or_email() {
    // Arrange
    let harness = Harness::new().await;
    let (admin, user) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = harness.fixture().await;
    seed::admin(&mut conn, admin).await;
    seed::user_with_credentials(&mut conn, user, NAME, EMAIL).await;
    let session = principal(
        admin,
        contracts::PrincipalKind::Session,
        contracts::Role::Admin,
    );
    let session_id = session.session_or_device_id;
    let caller = contracts::Actor::Admin(session);

    // Act
    let results = [
        harness
            .service
            .update_user(&caller, user, Some(Role::Admin), None)
            .await,
        harness
            .service
            .update_user(&caller, user, None, Some(AccountState::Disabled))
            .await,
        harness
            .service
            .set_quota(&caller, user, Some(5 * GIB))
            .await,
        harness.service.set_quota(&caller, user, None).await,
    ];

    // Assert
    assert!(results.iter().all(Result::is_ok));
    let rows = seed::audit_rows(&mut conn).await;
    assert_eq!(rows.len(), 4);
    let actions: Vec<Option<&str>> = rows.iter().map(|row| row.action.as_deref()).collect();
    assert_eq!(
        actions,
        [
            Some("role_admin"),
            Some("state_disabled"),
            Some("quota_limit_set"),
            Some("quota_limit_default")
        ]
    );
    for row in &rows {
        assert_eq!(row.event, "admin_action");
        assert_eq!(row.actor_account_id, Some(admin));
        assert_eq!(row.actor_session_id, Some(session_id));
        assert_eq!(row.subject_type.as_deref(), Some("account"));
        assert_eq!(row.subject_id, Some(user));
        assert!(!row.whole_row.contains(NAME), "{}", row.whole_row);
        assert!(!row.whole_row.contains("Ulrika"), "{}", row.whole_row);
        assert!(!row.whole_row.contains(EMAIL), "{}", row.whole_row);
        assert!(!row.whole_row.contains("example.test"), "{}", row.whole_row);
    }
    harness.finish().await;
}

#[tokio::test]
async fn an_operator_change_is_recorded_without_an_actor() {
    // Arrange
    let harness = Harness::new().await;
    let user = Uuid::new_v4();
    let mut conn = harness.fixture().await;
    seed::user_with_credentials(&mut conn, user, NAME, EMAIL).await;

    // Act
    let result = harness
        .service
        .update_user(&contracts::Actor::Operator, user, Some(Role::Admin), None)
        .await;

    // Assert
    assert!(result.is_ok());
    let rows = seed::audit_rows(&mut conn).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].actor_account_id, rows[0].actor_session_id),
        (None, None)
    );
    assert_eq!(rows[0].subject_id, Some(user));
    harness.finish().await;
}

#[tokio::test]
async fn a_failed_change_leaves_no_row() {
    // Arrange
    let harness = Harness::new().await;
    let (admin, user) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = harness.fixture().await;
    seed::admin(&mut conn, admin).await;
    seed::user_with_credentials(&mut conn, user, NAME, EMAIL).await;

    // Act
    let result = harness
        .service
        .set_quota(&admin_actor(admin), user, Some(-5))
        .await;

    // Assert
    assert!(result.is_err());
    assert!(seed::audit_rows(&mut conn).await.is_empty());
    harness.finish().await;
}
