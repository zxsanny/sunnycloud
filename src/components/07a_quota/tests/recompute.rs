//! AC-1, AC-2 and AC-74 of the quota recompute: drift of both counters against the supplied truth, reported
//! without change or repaired on request. The log half of AC-1 is in `recompute_log.rs`.

mod support;

use contracts::{Drift, QuotaAdmin, QuotaAdminError};
use sc_db::DbError;
use sqlx::Connection;
use support::{GIB, Harness, MIB, admin, initialized, quota_row};
use uuid::Uuid;

const LIVE: i64 = 600 * MIB;
const OPEN: i64 = 40 * MIB;

#[tokio::test]
async fn drift_of_both_columns_is_reported_separately_and_the_row_is_unchanged() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    initialized(&harness, GIB, id, Some(2 * GIB), LIVE + 777, OPEN - 5).await;
    let quota = admin(&harness.api, GIB);

    // Act
    let drift = quota
        .recompute_columns(id, LIVE, OPEN, false)
        .await
        .unwrap();
    let through_port = QuotaAdmin::recompute(&quota, id, LIVE, OPEN, false)
        .await
        .unwrap();

    // Assert
    assert_eq!(
        drift.used,
        Drift {
            stored: LIVE + 777,
            computed: LIVE
        }
    );
    assert_eq!(
        drift.reserved,
        Drift {
            stored: OPEN - 5,
            computed: OPEN
        }
    );
    assert!(drift.found());
    assert_eq!(through_port, drift.used);
    assert_eq!(
        quota_row(&mut conn, id).await,
        (Some(2 * GIB), LIVE + 777, OPEN - 5)
    );
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn repair_sets_the_supplied_values_and_the_recheck_reports_no_drift() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    initialized(&harness, GIB, id, Some(2 * GIB), LIVE + 777, OPEN - 5).await;
    let quota = admin(&harness.api, GIB);

    // Act
    let repaired = quota.recompute_columns(id, LIVE, OPEN, true).await.unwrap();
    let after_repair = quota_row(&mut conn, id).await;
    let recheck = quota
        .recompute_columns(id, LIVE, OPEN, false)
        .await
        .unwrap();

    // Assert
    assert!(repaired.found());
    assert_eq!(repaired.used.stored, LIVE + 777);
    assert_eq!(repaired.reserved.stored, OPEN - 5);
    assert_eq!(after_repair, (Some(2 * GIB), LIVE, OPEN));
    assert!(!recheck.found());
    assert_eq!(
        recheck.used,
        Drift {
            stored: LIVE,
            computed: LIVE
        }
    );
    assert_eq!(
        recheck.reserved,
        Drift {
            stored: OPEN,
            computed: OPEN
        }
    );
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn equal_values_report_no_drift_and_a_difference_in_either_column_alone_is_drift() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (equal, used_only, reserved_only) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    for id in [equal, used_only, reserved_only] {
        initialized(&harness, GIB, id, None, LIVE, OPEN).await;
    }
    let quota = admin(&harness.api, GIB);

    // Act
    let none = quota
        .recompute_columns(equal, LIVE, OPEN, false)
        .await
        .unwrap();
    let used = quota
        .recompute_columns(used_only, LIVE - 1, OPEN, false)
        .await
        .unwrap();
    let reserved = quota
        .recompute_columns(reserved_only, LIVE, OPEN + 1, false)
        .await
        .unwrap();

    // Assert
    assert!(!none.found());
    assert!(used.found());
    assert_eq!(
        used.used,
        Drift {
            stored: LIVE,
            computed: LIVE - 1
        }
    );
    assert_eq!(used.reserved.stored, used.reserved.computed);
    assert!(reserved.found());
    assert_eq!(
        reserved.reserved,
        Drift {
            stored: OPEN,
            computed: OPEN + 1
        }
    );
    assert_eq!(reserved.used.stored, reserved.used.computed);
    for id in [equal, used_only, reserved_only] {
        assert_eq!(quota_row(&mut conn, id).await, (None, LIVE, OPEN));
    }
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn repair_changes_only_the_named_account() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (target, other) = (Uuid::new_v4(), Uuid::new_v4());
    initialized(&harness, GIB, target, None, 1, 2).await;
    initialized(&harness, GIB, other, Some(GIB), 3, 4).await;
    let quota = admin(&harness.api, GIB);

    // Act
    quota.recompute_columns(target, 10, 20, true).await.unwrap();

    // Assert
    assert_eq!(quota_row(&mut conn, target).await, (None, 10, 20));
    assert_eq!(quota_row(&mut conn, other).await, (Some(GIB), 3, 4));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn an_account_without_a_row_is_not_found_with_or_without_repair() {
    // Arrange
    let harness = Harness::new().await;
    let quota = admin(&harness.api, GIB);
    let unknown = Uuid::new_v4();

    // Act
    let report = QuotaAdmin::recompute(&quota, unknown, 1, 1, false).await;
    let repair = QuotaAdmin::recompute(&quota, unknown, 1, 1, true).await;

    // Assert
    assert!(
        matches!(report, Err(QuotaAdminError::NotFound)),
        "{report:?}"
    );
    assert!(
        matches!(repair, Err(QuotaAdminError::NotFound)),
        "{repair:?}"
    );
    harness.finish().await;
}

#[tokio::test]
async fn a_failing_table_is_the_unexpected_tier_with_the_database_failure_as_source() {
    // Arrange
    let harness = Harness::new().await;
    let quota = admin(&harness.api, GIB);
    harness.db.admin_exec("DROP TABLE quota").await;

    // Act
    let result = QuotaAdmin::recompute(&quota, Uuid::new_v4(), 1, 1, true).await;

    // Assert
    let Err(QuotaAdminError::Unexpected(source)) = result else {
        panic!("expected the unexpected tier, got {result:?}");
    };
    assert!(source.downcast_ref::<DbError>().is_some());
    let message = QuotaAdminError::Unexpected(source).to_string();
    assert!(
        !message.contains("quota") && !message.contains("SQLSTATE"),
        "{message}"
    );
    harness.finish().await;
}
