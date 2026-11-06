//! AC-1, AC-2, AC-5 and AC-8 of the quota ledger, plus the contract's extreme inputs, the missing-row outcome
//! and account scoping. Every ledger call runs in a transaction of its own unless a test says otherwise.

mod support;

use contracts::{QuotaLedger, QuotaLedgerError, Reserved};
use sqlx::Connection;
use support::{GIB, Harness, MIB, account_with_quota, in_tx, ledger, quota_row};
use uuid::Uuid;

const DEFAULT: i64 = 10 * GIB;

struct Fixture {
    harness: Harness,
    account: Uuid,
}

async fn fixture(limit: Option<i64>, used: i64, reserved: i64) -> Fixture {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, limit, used, reserved).await;
    conn.close().await.expect("close fixture connection");
    Fixture { harness, account }
}

impl Fixture {
    async fn row(&self) -> (Option<i64>, i64, i64) {
        let mut conn = self.harness.fixture_conn().await;
        let row = quota_row(&mut conn, self.account).await;
        conn.close().await.expect("close fixture connection");
        row
    }

    async fn reserve(&self, bytes: i64) -> Result<Reserved, QuotaLedgerError> {
        in_tx(&self.harness.api, self.account, async |tx| {
            ledger(DEFAULT).reserve(tx, self.account, bytes).await
        })
        .await
    }

    async fn release(&self, bytes: i64) -> Result<(), QuotaLedgerError> {
        in_tx(&self.harness.api, self.account, async |tx| {
            ledger(DEFAULT)
                .release_reservation(tx, self.account, bytes)
                .await
        })
        .await
    }

    async fn adjust(&self, delta: i64) -> Result<(), QuotaLedgerError> {
        in_tx(&self.harness.api, self.account, async |tx| {
            ledger(DEFAULT).adjust_used(tx, self.account, delta).await
        })
        .await
    }
}

#[tokio::test]
async fn reserve_within_the_limit_twice_adds_to_reserved_only() {
    // Arrange
    let fx = fixture(Some(GIB), 300 * MIB, 0).await;

    // Act
    let first = fx.reserve(100 * MIB).await;
    let second = fx.reserve(100 * MIB).await;

    // Assert
    assert_eq!(first.unwrap().reserved_bytes, 100 * MIB);
    assert_eq!(second.unwrap().reserved_bytes, 200 * MIB);
    assert_eq!(fx.row().await, (Some(GIB), 300 * MIB, 200 * MIB));
    fx.harness.finish().await;
}

#[tokio::test]
async fn refused_reserve_changes_nothing_and_an_exact_fit_is_granted() {
    // Arrange
    let fx = fixture(Some(GIB), 800 * MIB, 100 * MIB).await;

    // Act
    let refused = fx.reserve(125 * MIB).await;
    let unchanged = fx.row().await;
    let exact = fx.reserve(124 * MIB).await;
    let one_more = fx.reserve(1).await;

    // Assert
    let Err(QuotaLedgerError::QuotaExceeded {
        limit,
        used,
        reserved,
    }) = refused
    else {
        panic!("expected QuotaExceeded, got {refused:?}");
    };
    assert_eq!((limit, used, reserved), (GIB, 800 * MIB, 100 * MIB));
    assert_eq!(unchanged, (Some(GIB), 800 * MIB, 100 * MIB));
    assert_eq!(exact.unwrap().reserved_bytes, 224 * MIB);
    assert!(matches!(
        one_more,
        Err(QuotaLedgerError::QuotaExceeded { .. })
    ));
    assert_eq!(fx.row().await, (Some(GIB), 800 * MIB, 224 * MIB));
    fx.harness.finish().await;
}

#[tokio::test]
async fn refusal_does_not_abort_the_callers_transaction() {
    // Arrange
    let fx = fixture(Some(GIB), GIB, 0).await;

    // Act
    let (refused, later) = in_tx(&fx.harness.api, fx.account, async |tx| {
        let ledger = ledger(DEFAULT);
        let refused = ledger.reserve(tx, fx.account, 1).await;
        let later = ledger.adjust_used(tx, fx.account, -MIB).await;
        (refused, later)
    })
    .await;

    // Assert
    assert!(matches!(
        refused,
        Err(QuotaLedgerError::QuotaExceeded { .. })
    ));
    later.unwrap();
    assert_eq!(fx.row().await, (Some(GIB), GIB - MIB, 0));
    fx.harness.finish().await;
}

#[tokio::test]
async fn signed_adjustments_apply_and_a_negative_result_is_an_invariant_violation() {
    // Arrange
    let fx = fixture(None, 100, 0).await;

    // Act
    let up = fx.adjust(50).await;
    let after_up = fx.row().await.1;
    let down = fx.adjust(-120).await;
    let after_down = fx.row().await.1;
    let below_zero = fx.adjust(-31).await;

    // Assert
    up.unwrap();
    down.unwrap();
    assert_eq!((after_up, after_down), (150, 30));
    let error = below_zero.expect_err("the result would be negative");
    assert!(matches!(error, QuotaLedgerError::InvariantViolation));
    for text in [error.to_string(), format!("{error:?}")] {
        for word in ["quota_bytes_check", "constraint", "violates", "SQLSTATE"] {
            assert!(!text.contains(word), "{word} in {text:?}");
        }
    }
    assert_eq!(fx.row().await.1, 30);
    fx.harness.finish().await;
}

#[tokio::test]
async fn invariant_violation_leaves_the_transaction_usable() {
    // Arrange
    let fx = fixture(None, 30, 0).await;

    // Act
    let (refused, later) = in_tx(&fx.harness.api, fx.account, async |tx| {
        let ledger = ledger(DEFAULT);
        let refused = ledger.adjust_used(tx, fx.account, -31).await;
        let later = ledger.adjust_used(tx, fx.account, 1).await;
        (refused, later)
    })
    .await;

    // Assert
    assert!(matches!(refused, Err(QuotaLedgerError::InvariantViolation)));
    later.unwrap();
    assert_eq!(fx.row().await.1, 31);
    fx.harness.finish().await;
}

#[tokio::test]
async fn account_without_a_limit_uses_the_installation_default() {
    // Arrange
    let fx = fixture(None, 0, 0).await;

    // Act
    let first = fx.reserve(5 * GIB).await;
    let second = fx.reserve(5 * GIB).await;
    let over = fx.reserve(1).await;

    // Assert
    first.unwrap();
    assert_eq!(second.unwrap().reserved_bytes, 10 * GIB);
    let Err(QuotaLedgerError::QuotaExceeded { limit, .. }) = over else {
        panic!("expected QuotaExceeded, got {over:?}");
    };
    assert_eq!(limit, 10 * GIB);
    assert_eq!(fx.row().await, (None, 0, 10 * GIB));
    fx.harness.finish().await;
}

#[tokio::test]
async fn account_limit_wins_over_the_default() {
    // Arrange
    let fx = fixture(Some(GIB), 0, 0).await;

    // Act
    let over = fx.reserve(GIB + 1).await;

    // Assert
    let Err(QuotaLedgerError::QuotaExceeded { limit, .. }) = over else {
        panic!("expected QuotaExceeded, got {over:?}");
    };
    assert_eq!(limit, GIB);
    fx.harness.finish().await;
}

#[tokio::test]
async fn extreme_inputs_return_typed_errors_and_change_nothing() {
    // Arrange
    let fx = fixture(Some(GIB), i64::MAX - 1, 5).await;

    // Act
    let huge_reserve = fx.reserve(i64::MAX).await;
    let negative_reserve = fx.reserve(-1).await;
    let negative_release = fx.release(-1).await;
    let past_max = fx.adjust(2).await;
    let min_delta = fx.adjust(i64::MIN).await;
    let max_delta = fx.adjust(i64::MAX).await;

    // Assert
    assert!(matches!(
        huge_reserve,
        Err(QuotaLedgerError::QuotaExceeded { .. })
    ));
    for result in [
        negative_reserve.map(drop),
        negative_release,
        past_max,
        min_delta,
        max_delta,
    ] {
        assert!(matches!(result, Err(QuotaLedgerError::InvariantViolation)));
    }
    assert_eq!(fx.row().await, (Some(GIB), i64::MAX - 1, 5));
    fx.harness.finish().await;
}

#[tokio::test]
async fn account_without_a_quota_row_is_an_unexpected_failure_naming_only_the_id() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    support::account(&mut conn, account).await;
    conn.close().await.expect("close fixture connection");
    let ledger = ledger(DEFAULT);

    // Act
    let results = in_tx(&harness.api, account, async |tx| {
        let reserve = ledger.reserve(tx, account, 1).await.map(drop);
        let release = ledger.release_reservation(tx, account, 1).await;
        let adjust = ledger.adjust_used(tx, account, 1).await;
        [reserve, release, adjust]
    })
    .await;

    // Assert
    for result in results {
        let Err(QuotaLedgerError::Unexpected(fault)) = result else {
            panic!("expected Unexpected, got {result:?}");
        };
        assert_eq!(
            fault.to_string(),
            format!("quota row missing for account {account}")
        );
        assert!(fault.source().is_none());
    }
    harness.finish().await;
}

#[tokio::test]
async fn calls_change_only_the_row_of_the_named_account() {
    // Arrange
    let fx = fixture(Some(GIB), 100, 50).await;
    let other = Uuid::new_v4();
    let mut conn = fx.harness.fixture_conn().await;
    account_with_quota(&mut conn, other, Some(GIB), 7, 3).await;
    conn.close().await.expect("close fixture connection");

    // Act
    fx.reserve(10).await.unwrap();
    fx.release(20).await.unwrap();
    fx.adjust(5).await.unwrap();

    // Assert
    assert_eq!(fx.row().await, (Some(GIB), 105, 40));
    let mut conn = fx.harness.fixture_conn().await;
    assert_eq!(quota_row(&mut conn, other).await, (Some(GIB), 7, 3));
    conn.close().await.expect("close fixture connection");
    fx.harness.finish().await;
}
