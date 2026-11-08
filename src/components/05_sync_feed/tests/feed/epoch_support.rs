//! Fixtures of the epoch admin tests: feed rows written as the schema owner, an admin over the fixture's pool and
//! a marker double.

use std::sync::Arc;

use sqlx::Connection;
use sunnycloud_sync_feed::PgEpochAdmin;
use uuid::Uuid;

use crate::marker_double::MarkerDouble;
use crate::support::{Fixture, START, account_with_feed};

pub fn start_seconds() -> i64 {
    START.as_unix_micros().div_euclid(1_000_000)
}

pub struct Epochs {
    pub fixture: Fixture,
    pub admin: PgEpochAdmin,
    pub marker: Arc<MarkerDouble>,
}

impl Epochs {
    pub async fn new(marker: MarkerDouble) -> Epochs {
        let fixture = Fixture::new().await;
        let marker = Arc::new(marker);
        let admin = PgEpochAdmin::new(fixture.api.clone(), marker.clone());
        Epochs {
            fixture,
            admin,
            marker,
        }
    }

    /// One account per entry of `epochs`, each with a feed row at that epoch; the ids in the same order.
    pub async fn accounts(&self, epochs: &[i64]) -> Vec<Uuid> {
        let mut conn = self.fixture.owner_conn().await;
        let mut ids = Vec::new();
        for epoch in epochs {
            let id = Uuid::new_v4();
            account_with_feed(&mut conn, id, *epoch).await;
            ids.push(id);
        }
        conn.close().await.expect("close owner connection");
        ids
    }

    pub async fn execute(&self, sql: &str) {
        let mut conn = self.fixture.owner_conn().await;
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(&mut conn)
            .await
            .expect("owner statement");
        conn.close().await.expect("close owner connection");
    }

    pub async fn epochs(&self) -> Vec<i64> {
        let mut conn = self.fixture.owner_conn().await;
        let epochs = sqlx::query_scalar("SELECT epoch FROM account_feeds ORDER BY account_id")
            .fetch_all(&mut conn)
            .await
            .expect("read the epochs");
        conn.close().await.expect("close owner connection");
        epochs
    }

    pub async fn seqs(&self) -> Vec<i64> {
        let mut conn = self.fixture.owner_conn().await;
        let seqs = sqlx::query_scalar("SELECT seq FROM account_feeds ORDER BY account_id")
            .fetch_all(&mut conn)
            .await
            .expect("read the seqs");
        conn.close().await.expect("close owner connection");
        seqs
    }

    /// The admin holds a clone of the pool; it goes before the fixture checks for leaked connections.
    pub async fn finish(self) {
        let Epochs { fixture, admin, .. } = self;
        drop(admin);
        fixture.finish().await;
    }
}
