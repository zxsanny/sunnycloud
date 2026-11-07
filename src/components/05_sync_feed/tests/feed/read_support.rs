//! Fixtures of the read tests: a stamped account, node edits made the way the tree makes them (plain SQL as the
//! schema owner, then a real stamp), and a reader over the fixture's pool.

use contracts::{ChangeFeedError, ChangePage, Cursor};
use sqlx::Connection;
use sunnycloud_sync_feed::{FeedConfig, PgFeedReader};
use uuid::Uuid;

use crate::stamp::{Stamped, account_with_nodes};

pub const EPOCH: i64 = 7;

pub fn cursor(seq: i64) -> Cursor {
    Cursor { epoch: EPOCH, seq }
}

pub fn feed_config(default: u64, ceiling: Option<u64>) -> FeedConfig {
    FeedConfig {
        longpoll_wait_max: std::time::Duration::from_secs(30),
        longpoll_auth_recheck: std::time::Duration::from_secs(2),
        feed_safety_recheck: std::time::Duration::from_secs(10),
        feed_page_limit_default: default,
        feed_page_limit_max: ceiling,
        max_holds_per_principal: 4,
    }
}

impl Stamped {
    pub fn reader(&self) -> PgFeedReader {
        PgFeedReader::new(self.fixture.api.clone(), &feed_config(500, None))
    }

    pub async fn read_from(&self, seq: i64) -> Result<ChangePage, ChangeFeedError> {
        self.reader()
            .read_page(self.account, cursor(seq), None)
            .await
    }

    pub async fn set_name(&self, node: Uuid, name: &str) {
        self.execute(
            sqlx::query("UPDATE nodes SET name = $2, name_key = $2 WHERE id = $1")
                .bind(node)
                .bind(name),
        )
        .await;
    }

    pub async fn set_tombstone(&self, node: Uuid, tombstone: bool) {
        self.execute(
            sqlx::query(
                "UPDATE nodes SET deleted_at = CASE WHEN $2 THEN 'epoch'::timestamptz END WHERE id = $1",
            )
            .bind(node)
            .bind(tombstone),
        )
        .await;
    }

    /// A file under the root with one committed revision; returns the node and the revision id.
    pub async fn add_file(&self, size: i64, hash_byte: u8) -> (Uuid, Uuid) {
        let (node, blob, revision) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut conn = self.fixture.owner_conn().await;
        let statements = [
            sqlx::query(
                "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'file', 'doc.bin', 'doc.bin', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
            )
            .bind(node)
            .bind(self.account)
            .bind(self.root),
            sqlx::query(
                "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
                 VALUES ($1, $2, 'committed', $3, '\\x00', 1, 'epoch'::timestamptz)",
            )
            .bind(blob)
            .bind(self.account)
            .bind(size),
            sqlx::query(
                "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, '\\x', 'epoch'::timestamptz)",
            )
            .bind(revision)
            .bind(self.account)
            .bind(node)
            .bind(blob)
            .bind(size)
            .bind(vec![hash_byte; 32]),
            sqlx::query("UPDATE nodes SET current_revision_id = $2 WHERE id = $1")
                .bind(node)
                .bind(revision),
        ];
        for statement in statements {
            statement
                .execute(&mut conn)
                .await
                .expect("insert file fixture");
        }
        conn.close().await.expect("close owner connection");
        (node, revision)
    }

    /// Sets the counter row directly: a feed that has already lived through `seq` stamps and a purge.
    pub async fn set_feed(&self, seq: i64, purge_horizon: i64) {
        self.execute(
            sqlx::query(
                "UPDATE account_feeds SET seq = $2, purge_horizon = $3 WHERE account_id = $1",
            )
            .bind(self.account)
            .bind(seq)
            .bind(purge_horizon),
        )
        .await;
    }

    async fn execute(
        &self,
        statement: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) {
        let mut conn = self.fixture.owner_conn().await;
        let done = statement.execute(&mut conn).await.expect("fixture update");
        assert_eq!(done.rows_affected(), 1);
        conn.close().await.expect("close owner connection");
    }
}

/// The data of AC-1 and AC-2: node A stamped at 1 and again at 5 after a rename, `b` at 2, a file at 3, `c` at
/// 4, folder `f` tombstoned at 6, and a batch of `batch` nodes sharing seq 7.
pub struct Scenario {
    pub stamped: Stamped,
    pub a: Uuid,
    pub f: Uuid,
    pub b: Uuid,
    pub c: Uuid,
    pub file: Uuid,
    pub revision: Uuid,
    pub batch: Vec<Uuid>,
}

pub const FILE_SIZE: i64 = 4096;
pub const FILE_HASH_BYTE: u8 = 0xAB;

pub async fn scenario(batch: usize) -> Scenario {
    let (stamped, ids) = account_with_nodes(4 + i32::try_from(batch).expect("batch fits")).await;
    let (a, f, b, c) = (ids[0], ids[1], ids[2], ids[3]);
    let (file, revision) = stamped.add_file(FILE_SIZE, FILE_HASH_BYTE).await;
    stamped.stamp_committed(&[a]).await.expect("seq 1");
    stamped.stamp_committed(&[b]).await.expect("seq 2");
    stamped.stamp_committed(&[file]).await.expect("seq 3");
    stamped.stamp_committed(&[c]).await.expect("seq 4");
    stamped.set_name(a, "renamed").await;
    stamped.stamp_committed(&[a]).await.expect("seq 5");
    stamped.set_tombstone(f, true).await;
    stamped.stamp_committed(&[f]).await.expect("seq 6");
    let batch = ids[4..].to_vec();
    if !batch.is_empty() {
        let seq = stamped.stamp_committed(&batch).await.expect("seq 7");
        assert_eq!(seq, 7);
    }
    Scenario {
        stamped,
        a,
        f,
        b,
        c,
        file,
        revision,
        batch,
    }
}
