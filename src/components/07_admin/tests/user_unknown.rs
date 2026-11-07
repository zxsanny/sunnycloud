//! AC-5: an unknown account is `NotFound` and nothing is written.

mod support;

use contracts::{AccountState, AdminError, Role};
use sunnycloud_admin::AdminCallError;
use support::harness::{Harness, admin_actor};
use support::seed;
use uuid::Uuid;

#[tokio::test]
async fn update_and_set_quota_on_unknown_accounts_are_not_found_and_write_nothing() {
    // Arrange
    let harness = Harness::new().await;
    let admin = Uuid::new_v4();
    let mut conn = harness.fixture().await;
    seed::admin(&mut conn, admin).await;
    let caller = admin_actor(admin);
    let accounts_before = seed::row_count(&mut conn, "accounts").await;
    let quota_before = seed::row_count(&mut conn, "quota").await;

    for unknown in [Uuid::nil(), Uuid::new_v4()] {
        // Act
        let role = harness
            .service
            .update_user(&caller, unknown, Some(Role::Admin), None)
            .await;
        let state = harness
            .service
            .update_user(&caller, unknown, None, Some(AccountState::Disabled))
            .await;
        let quota = harness
            .service
            .set_quota(&caller, unknown, Some(1 << 30))
            .await;

        // Assert
        for result in [role, state, quota] {
            assert!(
                matches!(result, Err(AdminCallError::Failed(AdminError::NotFound))),
                "{unknown}: {result:?}"
            );
        }
    }
    assert_eq!(
        harness
            .accounts
            .mutations
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(seed::row_count(&mut conn, "audit_log").await, 0);
    assert_eq!(
        seed::row_count(&mut conn, "accounts").await,
        accounts_before
    );
    assert_eq!(seed::row_count(&mut conn, "quota").await, quota_before);
    harness.finish().await;
}
