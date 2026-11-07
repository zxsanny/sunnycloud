//! AC-3: the quota limit passes through to 07a, and an invalid one changes nothing.

mod support;

use contracts::{AdminError, QuotaAdmin};
use sunnycloud_admin::AdminCallError;
use support::harness::{Harness, admin_actor};
use support::quota_double::DEFAULT_LIMIT;
use support::seed;
use uuid::Uuid;

const GIB: i64 = 1 << 30;

async fn scene() -> (Harness, Uuid, Uuid) {
    let harness = Harness::new().await;
    let (admin, user) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = harness.fixture().await;
    seed::admin(&mut conn, admin).await;
    seed::user_with_credentials(&mut conn, user, "Ulrika Utest", "ulrika@example.test").await;
    drop(conn);
    (harness, admin, user)
}

#[tokio::test]
async fn the_limit_is_set_to_5_gib_and_then_back_to_the_installation_default() {
    // Arrange
    let (harness, admin, user) = scene().await;
    let caller = admin_actor(admin);

    // Act
    let set = harness
        .service
        .set_quota(&caller, user, Some(5 * GIB))
        .await;
    let after_set = harness.quota.view(user).await.expect("view after set");
    let reset = harness.service.set_quota(&caller, user, None).await;
    let after_reset = harness.quota.view(user).await.expect("view after reset");

    // Assert
    assert!(set.is_ok() && reset.is_ok());
    assert_eq!(
        (after_set.limit_bytes, after_set.effective_limit),
        (Some(5 * GIB), 5 * GIB)
    );
    assert_eq!(
        (after_reset.limit_bytes, after_reset.effective_limit),
        (None, DEFAULT_LIMIT)
    );
    harness.finish().await;
}

#[tokio::test]
async fn an_out_of_range_limit_is_invalid_and_changes_nothing() {
    // Arrange
    let (harness, admin, user) = scene().await;
    let caller = admin_actor(admin);
    harness
        .service
        .set_quota(&caller, user, Some(GIB))
        .await
        .expect("set a valid limit");
    let audit_before = seed::row_count(&mut harness.fixture().await, "audit_log").await;

    // Act
    let negative = harness.service.set_quota(&caller, user, Some(-1)).await;
    let too_large = harness
        .service
        .set_quota(&caller, user, Some((1 << 53) + 1))
        .await;

    // Assert
    let mut conn = harness.fixture().await;
    for result in [negative, too_large] {
        assert!(matches!(
            result,
            Err(AdminCallError::Failed(AdminError::InvalidLimit))
        ));
    }
    assert_eq!(seed::limit_of(&mut conn, user).await, Some(GIB));
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, audit_before);
    harness.finish().await;
}

#[tokio::test]
async fn the_bounds_of_the_range_are_accepted() {
    // Arrange
    let (harness, admin, user) = scene().await;
    let caller = admin_actor(admin);

    // Act
    let zero = harness.service.set_quota(&caller, user, Some(0)).await;
    let top = harness
        .service
        .set_quota(&caller, user, Some(1 << 53))
        .await;

    // Assert
    assert!(zero.is_ok() && top.is_ok());
    assert_eq!(
        seed::limit_of(&mut harness.fixture().await, user).await,
        Some(1 << 53)
    );
    harness.finish().await;
}
