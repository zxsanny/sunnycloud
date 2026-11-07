//! AC-2 of the admin lifecycle: per-user limits, including below current use. Refusal is checked through
//! `reserve` of the ledger.

mod support;

use contracts::{QuotaAdminError, QuotaLedger, QuotaLedgerError, QuotaView};
use sqlx::Connection;
use sunnycloud_quota::MAX_LIMIT_BYTES;
use support::{GIB, Harness, MIB, admin, in_tx, initialized, ledger, quota_row};
use uuid::Uuid;

const DEFAULT: i64 = 10 * GIB;

async fn reserve(harness: &Harness, id: Uuid, bytes: i64) -> Result<(), QuotaLedgerError> {
    in_tx(&harness.api, id, async |tx| {
        ledger(DEFAULT).reserve(tx, id, bytes).await.map(drop)
    })
    .await
}

#[tokio::test]
async fn limit_bounds_zero_and_two_to_the_53_are_accepted_and_values_outside_are_refused() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    initialized(&harness, DEFAULT, id, None, 0, 0).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let inside = [0, 1, MAX_LIMIT_BYTES];
    let outside = [-1, MAX_LIMIT_BYTES + 1, i64::MIN, i64::MAX];
    let mut accepted = Vec::new();
    for limit in inside {
        accepted.push(quota.set_limit(id, Some(limit)).await.is_ok());
    }
    let refused: Vec<_> = {
        let mut results = Vec::new();
        for limit in outside {
            results.push(quota.set_limit(id, Some(limit)).await);
        }
        results
    };

    // Assert
    assert_eq!(accepted, vec![true; 3]);
    assert!(
        refused
            .iter()
            .all(|result| matches!(result, Err(QuotaAdminError::InvalidLimit)))
    );
    assert_eq!(
        quota.view(id).await.unwrap().limit_bytes,
        Some(MAX_LIMIT_BYTES)
    );
    harness.finish().await;
}

#[tokio::test]
async fn a_limit_below_use_keeps_the_data_blocks_new_reservations_and_none_restores_the_default() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    initialized(&harness, DEFAULT, id, None, 600 * MIB, 0).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let lowered = quota.set_limit(id, Some(500 * MIB)).await;
    let view_lowered = quota.view(id).await.unwrap();
    let refused = reserve(&harness, id, 1).await;
    let row_after_refusal = quota_row(&mut conn, id).await;
    let restored = quota.set_limit(id, None).await;
    let view_restored = quota.view(id).await.unwrap();
    let granted = reserve(&harness, id, 1).await;
    let zero = quota.set_limit(id, Some(0)).await;
    let refused_at_zero = reserve(&harness, id, 1).await;

    // Assert
    lowered.unwrap();
    assert_eq!(
        view_lowered,
        QuotaView {
            limit_bytes: Some(500 * MIB),
            effective_limit: 500 * MIB,
            used: 600 * MIB,
            reserved: 0
        }
    );
    assert!(matches!(
        refused,
        Err(QuotaLedgerError::QuotaExceeded { limit, used, reserved: 0 })
            if limit == 500 * MIB && used == 600 * MIB
    ));
    assert_eq!(row_after_refusal, (Some(500 * MIB), 600 * MIB, 0));
    restored.unwrap();
    assert_eq!(
        view_restored,
        QuotaView {
            limit_bytes: None,
            effective_limit: DEFAULT,
            used: 600 * MIB,
            reserved: 0
        }
    );
    granted.unwrap();
    zero.unwrap();
    assert!(matches!(
        refused_at_zero,
        Err(QuotaLedgerError::QuotaExceeded { limit: 0, .. })
    ));
    assert_eq!(quota_row(&mut conn, id).await, (Some(0), 600 * MIB, 1));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn invalid_limits_change_nothing_and_an_unknown_account_is_not_found() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    initialized(&harness, DEFAULT, id, Some(GIB), 10, 20).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let negative = quota.set_limit(id, Some(-1)).await;
    let too_large = quota.set_limit(id, Some(MAX_LIMIT_BYTES + 1)).await;
    let unknown = quota.set_limit(Uuid::new_v4(), Some(GIB)).await;
    let unknown_default = quota.set_limit(Uuid::new_v4(), None).await;

    // Assert
    assert!(matches!(negative, Err(QuotaAdminError::InvalidLimit)));
    assert!(matches!(too_large, Err(QuotaAdminError::InvalidLimit)));
    assert!(matches!(unknown, Err(QuotaAdminError::NotFound)));
    assert!(matches!(unknown_default, Err(QuotaAdminError::NotFound)));
    assert_eq!(quota_row(&mut conn, id).await, (Some(GIB), 10, 20));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}
