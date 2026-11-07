//! AC-4: a missing feed row becomes `AccountFeedMissing` and a database failure the unexpected tier, and no
//! message holds SQL or constraint text.

use std::error::Error;

use contracts::{ChangeFeedError, FeedHorizonError, FeedStampError};
use sc_db::{DbError, RunError, Scope};
use sunnycloud_sync_feed::{account_feed_missing, horizon_error, read_error, stamp_error};
use uuid::Uuid;

use crate::support::Fixture;

const FORBIDDEN_TEXT: [&str; 7] = [
    "account_feeds",
    "SQLSTATE",
    "violates",
    "constraint",
    "INSERT",
    "SELECT",
    "foreign key",
];

fn assert_no_database_text(message: &str) {
    for word in FORBIDDEN_TEXT {
        assert!(!message.contains(word), "{word} in {message:?}");
    }
}

fn closure_or_db(error: RunError<DbError>) -> DbError {
    match error {
        RunError::Closure(error) | RunError::Db(error) => error,
    }
}

#[tokio::test]
async fn account_without_a_feed_row_is_account_feed_missing() {
    // Arrange
    let fixture = Fixture::new().await;
    let account = Uuid::new_v4();

    // Act
    let row: Option<i64> = fixture
        .api
        .run(Scope::Account(account), async |tx| {
            sqlx::query_scalar("SELECT seq FROM account_feeds WHERE account_id = $1")
                .bind(account)
                .fetch_optional(tx.conn())
                .await
        })
        .await
        .expect("scoped read");
    let error = account_feed_missing(account);

    // Assert
    assert_eq!(row, None);
    assert!(matches!(error, FeedStampError::AccountFeedMissing));
    assert_no_database_text(&error.to_string());
    assert_eq!(error.to_string(), "account feed missing");
    fixture.finish().await;
}

#[tokio::test]
async fn lost_connection_is_the_unexpected_tier_in_all_three_ports() {
    // Arrange
    let fixture = Fixture::new().await;
    let account = Uuid::new_v4();

    // Act
    let result = fixture
        .api
        .run(Scope::Account(account), async |tx| {
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(tx.conn())
                .await?;
            fixture
                .db
                .admin_exec(&format!("SELECT pg_terminate_backend({pid})"))
                .await;
            sqlx::query("UPDATE account_feeds SET seq = seq + 1 WHERE account_id = $1")
                .bind(account)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    // Assert
    let cause = closure_or_db(result.expect_err("the connection is gone"));
    let stamp = stamp_error(cause.clone());
    let horizon = horizon_error(cause.clone());
    let read = read_error(cause.clone());
    let FeedStampError::Unexpected(source) = &stamp else {
        panic!("expected the unexpected tier, got {stamp:?}");
    };
    assert_eq!(source.to_string(), cause.to_string());
    assert!(matches!(horizon, FeedHorizonError::Unexpected(_)));
    assert!(matches!(read, ChangeFeedError::Unexpected(_)));
    for message in [stamp.to_string(), horizon.to_string(), read.to_string()] {
        assert_eq!(message, "unexpected failure");
        assert_no_database_text(&message);
    }
    fixture.finish().await;
}

#[tokio::test]
async fn constraint_violation_is_the_unexpected_tier_without_constraint_text() {
    // Arrange
    let fixture = Fixture::new().await;
    let unknown_account = Uuid::new_v4();

    // Act
    let result = fixture
        .api
        .run(Scope::Account(unknown_account), async |tx| {
            sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 1)")
                .bind(unknown_account)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(())
        })
        .await;

    // Assert
    let cause = closure_or_db(result.expect_err("the account does not exist"));
    assert_eq!(cause, DbError::ForeignKeyViolation);
    let error = stamp_error(cause);
    assert_eq!(error.to_string(), "unexpected failure");
    assert_no_database_text(&error.to_string());
    assert!(error.source().is_some());
    fixture.finish().await;
}
