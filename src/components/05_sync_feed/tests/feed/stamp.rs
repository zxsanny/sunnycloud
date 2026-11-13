//! AC-1 to AC-4: the stamp on a real PostgreSQL 18. Account, feed row and node rows are fixture SQL run as the
//! schema owner; the stamp runs as `sunnycloud` inside `Db::run`.

use contracts::{FeedStamp, FeedStampError, Seq, TouchedNodes};
use sc_db::{DbError, RunError, Scope};
use sqlx::{Connection, PgConnection};
use sunnycloud_sync_feed::{EmptyTouchedNodes, PgFeedStamp, touched_nodes};
use uuid::Uuid;

use crate::support::{Fixture, account, account_with_feed, insert_folders, root_node};
use sc_testdb::statements::StatementLog;

const NODES: i32 = 600;
const BATCH: usize = 500;

pub struct Stamped {
    pub fixture: Fixture,
    pub account: Uuid,
    pub root: Uuid,
}

pub async fn account_with_nodes(nodes: i32) -> (Stamped, Vec<Uuid>) {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    account_with_feed(&mut conn, account, 7).await;
    let root = root_node(&mut conn, account).await;
    let ids = insert_folders(&mut conn, account, root, nodes)
        .await
        .expect("insert node fixtures");
    conn.close().await.expect("close owner connection");
    (
        Stamped {
            fixture,
            account,
            root,
        },
        ids,
    )
}

impl Stamped {
    pub async fn stamp_committed(&self, nodes: &[Uuid]) -> Result<Seq, FeedStampError> {
        let touched = TouchedNodes(nodes.to_vec());
        self.fixture
            .api
            .run(Scope::Account(self.account), async |tx| {
                PgFeedStamp.stamp(tx, self.account, touched).await
            })
            .await
            .map_err(|error| match error {
                RunError::Closure(error) => error,
                RunError::Db(error) => FeedStampError::Unexpected(Box::new(error)),
            })
    }

    pub async fn feed_seq(&self, conn: &mut PgConnection) -> i64 {
        sqlx::query_scalar("SELECT seq FROM account_feeds WHERE account_id = $1")
            .bind(self.account)
            .fetch_one(conn)
            .await
            .expect("read the counter")
    }

    pub async fn nodes_with_seq(&self, conn: &mut PgConnection, seq: i64) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM nodes WHERE account_id = $1 AND seq = $2")
            .bind(self.account)
            .bind(seq)
            .fetch_one(conn)
            .await
            .expect("count nodes")
    }
}

#[tokio::test]
async fn one_node_then_five_hundred_share_a_seq_each_and_untouched_nodes_keep_theirs() {
    // Arrange
    let (stamped, ids) = account_with_nodes(NODES).await;
    let log = StatementLog::install();
    let batch = &ids[1..=BATCH];

    // Act
    let first = stamped.stamp_committed(&ids[..1]).await;
    let first_statements = log.take_data_statements();
    let second = stamped.stamp_committed(batch).await;
    let second_statements = log.take_data_statements();

    // Assert
    assert_eq!(first.expect("first stamp"), 1);
    assert_eq!(second.expect("second stamp"), 2);
    for statements in [&first_statements, &second_statements] {
        assert_eq!(statements.len(), 1, "{statements:?}");
        assert!(statements[0].starts_with("WITH"), "{}", statements[0]);
        assert!(statements[0].contains("UPDATE account_feeds"));
        assert!(statements[0].contains("UPDATE nodes"));
    }
    let mut conn = stamped.fixture.owner_conn().await;
    assert_eq!(stamped.feed_seq(&mut conn).await, 2);
    assert_eq!(stamped.nodes_with_seq(&mut conn, 2).await, 500);
    assert_eq!(stamped.nodes_with_seq(&mut conn, 1).await, 1);
    let untouched = i64::from(NODES) + 1 - 500 - 1;
    assert_eq!(stamped.nodes_with_seq(&mut conn, 0).await, untouched);
    let stamped_ok: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE id = $1 AND seq = 1 \
         OR id = ANY($2) AND seq = 2 OR id = $3 AND seq = 0",
    )
    .bind(ids[0])
    .bind(batch)
    .bind(stamped.root)
    .fetch_one(&mut conn)
    .await
    .expect("count by identity");
    assert_eq!(stamped_ok, 502);
    conn.close().await.expect("close owner connection");
    stamped.fixture.finish().await;
}

pub enum Attempt {
    Rollback,
    Failed(FeedStampError),
    Db(DbError),
}

impl From<FeedStampError> for Attempt {
    fn from(error: FeedStampError) -> Self {
        Attempt::Failed(error)
    }
}

impl From<sqlx::Error> for Attempt {
    fn from(error: sqlx::Error) -> Self {
        Attempt::Db(error.into())
    }
}

#[tokio::test]
async fn a_rolled_back_stamp_leaves_the_counter_and_the_next_stamp_gets_the_next_integer() {
    // Arrange
    let (stamped, ids) = account_with_nodes(2).await;
    let committed = stamped.stamp_committed(&ids[..1]).await.expect("first");
    let mut conn = stamped.fixture.owner_conn().await;

    // Act
    let rolled_back = stamped
        .fixture
        .api
        .run(Scope::Account(stamped.account), async |tx| {
            let seq = PgFeedStamp
                .stamp(tx, stamped.account, TouchedNodes(vec![ids[1]]))
                .await?;
            assert_eq!(seq, 2);
            Err::<Seq, Attempt>(Attempt::Rollback)
        })
        .await;
    let after_rollback = stamped.feed_seq(&mut conn).await;
    let node_after_rollback = stamped.nodes_with_seq(&mut conn, 2).await;
    let next = stamped.stamp_committed(&ids[1..]).await;

    // Assert
    assert_eq!(committed, 1);
    assert!(matches!(
        rolled_back,
        Err(RunError::Closure(Attempt::Rollback))
    ));
    assert_eq!(after_rollback, 1);
    assert_eq!(node_after_rollback, 0);
    assert_eq!(next.expect("next stamp"), 2);
    assert_eq!(stamped.feed_seq(&mut conn).await, 2);
    assert_eq!(stamped.nodes_with_seq(&mut conn, 2).await, 1);
    conn.close().await.expect("close owner connection");
    stamped.fixture.finish().await;
}

#[tokio::test]
async fn an_account_without_a_feed_row_is_account_feed_missing_and_nothing_is_stamped() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account_id = Uuid::new_v4();
    account(&mut conn, account_id).await;
    let root = root_node(&mut conn, account_id).await;
    let stamped = Stamped {
        fixture,
        account: account_id,
        root,
    };

    // Act
    let result = stamped.stamp_committed(&[root]).await;

    // Assert
    assert!(
        matches!(result, Err(FeedStampError::AccountFeedMissing)),
        "{result:?}"
    );
    assert_eq!(stamped.nodes_with_seq(&mut conn, 0).await, 1);
    conn.close().await.expect("close owner connection");
    stamped.fixture.finish().await;
}

#[test]
fn an_empty_list_is_refused_when_the_touched_set_is_built() {
    // Arrange
    let nothing: Vec<Uuid> = Vec::new();
    let node = Uuid::new_v4();

    // Act
    let empty = touched_nodes(nothing);
    let one = touched_nodes(vec![node]);

    // Assert
    assert!(matches!(empty, Err(EmptyTouchedNodes)));
    assert_eq!(one.expect("one node").0, vec![node]);
}

#[tokio::test]
async fn an_empty_set_built_by_hand_is_refused_by_the_stamp_before_the_counter_moves() {
    // Arrange
    let (stamped, _) = account_with_nodes(1).await;
    let mut conn = stamped.fixture.owner_conn().await;

    // Act
    let result = stamped.stamp_committed(&[]).await;

    // Assert
    let Err(FeedStampError::Unexpected(source)) = result else {
        panic!("expected the unexpected tier, got {result:?}");
    };
    assert_eq!(source.to_string(), "touched nodes are empty");
    assert_eq!(stamped.feed_seq(&mut conn).await, 0);
    conn.close().await.expect("close owner connection");
    stamped.fixture.finish().await;
}
