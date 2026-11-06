//! AC-4: database failures raised inside the crate convert into the declared ledger errors, and no message
//! holds SQL or constraint text.

mod support;

use contracts::QuotaLedgerError;
use sc_db::{DbError, Scope};
use sqlx::Connection;
use sunnycloud_quota::{db_error, ledger_error};
use support::{Harness, account_with_quota};
use uuid::Uuid;

const FORBIDDEN_TEXT: [&str; 6] = [
    "quota_bytes_check",
    "SQLSTATE",
    "violates",
    "UPDATE",
    "used_bytes",
    "constraint",
];

fn assert_no_database_text(error: &QuotaLedgerError) {
    let message = error.to_string();
    for word in FORBIDDEN_TEXT {
        assert!(!message.contains(word), "{word} in {message:?}");
    }
}

#[tokio::test]
async fn negative_used_total_from_the_database_is_an_invariant_violation() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, None, 5, 0).await;
    conn.close().await.expect("close fixture connection");

    // Act
    let result = harness
        .api
        .run(Scope::Account(account), async |tx| {
            sqlx::query("UPDATE quota SET used_bytes = used_bytes - 10 WHERE account_id = $1")
                .bind(account)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    // Assert
    let error = ledger_error(db_error(result.expect_err("the CHECK refuses the update")));
    assert!(matches!(error, QuotaLedgerError::InvariantViolation));
    assert_no_database_text(&error);
    harness.finish().await;
}

#[tokio::test]
async fn lost_connection_is_the_unexpected_tier_without_database_text() {
    // Arrange
    let harness = Harness::new().await;
    let account = Uuid::new_v4();

    // Act
    let result = harness
        .api
        .run(Scope::Account(account), async |tx| {
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(tx.conn())
                .await?;
            harness
                .db
                .admin_exec(&format!("SELECT pg_terminate_backend({pid})"))
                .await;
            sqlx::query("UPDATE quota SET used_bytes = 1 WHERE account_id = $1")
                .bind(account)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    // Assert
    let cause = db_error(result.expect_err("the connection is gone"));
    let error = ledger_error(cause.clone());
    let QuotaLedgerError::Unexpected(fault) = &error else {
        panic!("expected the unexpected tier, got {error:?}");
    };
    assert_eq!(fault.to_string(), cause.to_string());
    assert_no_database_text(&error);
    harness.finish().await;
}
