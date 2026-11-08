//! AC-1 and AC-2 (IT-05-12): the purge horizon on a real PostgreSQL 18. The horizon is raised through a
//! transaction of `Db::run`, as 02's purge does; the cursor rules are judged by the real reader.

use contracts::{ChangeFeedError, FeedHorizon, FeedHorizonError, ResyncReason};
use sc_db::{RunError, Scope};
use sqlx::Connection;
use sunnycloud_sync_feed::PgFeedHorizon;
use uuid::Uuid;

use crate::stamp::{Stamped, account_with_nodes};

impl Stamped {
    async fn raise(&self, account: Uuid, seq: i64) -> Result<(), FeedHorizonError> {
        self.fixture
            .api
            .run(Scope::Account(account), async |tx| {
                PgFeedHorizon.raise_purge_horizon(tx, account, seq).await
            })
            .await
            .map_err(|error| match error {
                RunError::Closure(error) => error,
                RunError::Db(error) => FeedHorizonError::Unexpected(Box::new(error)),
            })
    }

    async fn horizon(&self) -> i64 {
        let mut conn = self.fixture.owner_conn().await;
        let horizon =
            sqlx::query_scalar("SELECT purge_horizon FROM account_feeds WHERE account_id = $1")
                .bind(self.account)
                .fetch_one(&mut conn)
                .await
                .expect("read the horizon");
        conn.close().await.expect("close owner connection");
        horizon
    }
}

fn is_below_horizon(result: Result<contracts::ChangePage, ChangeFeedError>) -> bool {
    matches!(
        result,
        Err(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::BelowPurgeHorizon
        })
    )
}

#[tokio::test]
async fn a_committed_raise_makes_cursors_below_it_resync_and_cursors_at_it_continue() {
    // Arrange
    let stamped = account_with_nodes(0).await.0;
    stamped.set_feed(50, 0).await;

    // Act
    stamped.raise(stamped.account, 30).await.expect("raise");

    // Assert
    assert_eq!(stamped.horizon().await, 30);
    assert!(is_below_horizon(stamped.read_from(29).await));
    assert!(stamped.read_from(30).await.is_ok());
    stamped.fixture.finish().await;
}

#[tokio::test]
async fn raising_the_horizon_of_an_account_without_a_feed_row_is_not_an_error() {
    // Arrange
    let stamped = account_with_nodes(0).await.0;

    // Act
    let result = stamped.raise(Uuid::new_v4(), 30).await;

    // Assert
    assert!(result.is_ok());
    assert_eq!(stamped.horizon().await, 0);
    stamped.fixture.finish().await;
}

#[tokio::test]
async fn a_raise_in_a_transaction_that_rolls_back_leaves_the_horizon_unchanged() {
    // Arrange
    let stamped = account_with_nodes(0).await.0;
    stamped.set_feed(50, 0).await;

    // Act
    let outcome = stamped
        .fixture
        .api
        .run(Scope::Account(stamped.account), async |tx| {
            PgFeedHorizon
                .raise_purge_horizon(tx, stamped.account, 30)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Err::<(), _>(std::io::Error::other("purge failed after the horizon"))
        })
        .await;

    // Assert
    assert!(matches!(outcome, Err(RunError::Closure(_))));
    assert_eq!(stamped.horizon().await, 0);
    assert!(stamped.read_from(0).await.is_ok());
    stamped.fixture.finish().await;
}

#[tokio::test]
async fn a_lower_seq_keeps_the_horizon_and_an_equal_or_higher_seq_moves_it() {
    // Arrange
    let stamped = account_with_nodes(0).await.0;
    stamped.set_feed(50, 30).await;

    // Act
    let lower = stamped.raise(stamped.account, 20).await;
    let after_lower = stamped.horizon().await;
    let equal = stamped.raise(stamped.account, 30).await;
    let after_equal = stamped.horizon().await;
    let higher = stamped.raise(stamped.account, 40).await;
    let after_higher = stamped.horizon().await;

    // Assert
    assert!(lower.is_ok() && equal.is_ok() && higher.is_ok());
    assert_eq!((after_lower, after_equal, after_higher), (30, 30, 40));
    assert!(is_below_horizon(stamped.read_from(35).await));
    stamped.fixture.finish().await;
}
