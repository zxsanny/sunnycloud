//! A bump logs one INFO line. This binary holds this one test: the capturing subscriber is installed on the test's
//! thread, and a scoped default races with the callsite interest cache when other tests run in parallel.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{EpochAdmin, EpochMarker, EpochMarkerError, async_trait};
use sc_clock::Timestamp;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sc_testdb::{MIGRATOR, START, TestDb, clock_at_start};
use sqlx::Connection;
use sunnycloud_sync_feed::PgEpochAdmin;
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

struct AcceptingMarker;

#[async_trait]
impl EpochMarker for AcceptingMarker {
    async fn write(&self, _epoch: i64) -> Result<(), EpochMarkerError> {
        Ok(())
    }

    async fn read(&self) -> Result<Option<i64>, EpochMarkerError> {
        Ok(None)
    }
}

#[tokio::test]
async fn a_bump_logs_one_info_line() {
    // Arrange
    let db = TestDb::clone_template().await;
    let api = db.api().await;
    let account = Uuid::new_v4();
    let mut conn = db.conn_as(MIGRATOR).await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(account)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert account");
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, 7)")
        .bind(account)
        .execute(&mut conn)
        .await
        .expect("insert feed row");
    conn.close().await.expect("close owner connection");
    let admin = PgEpochAdmin::new(api.clone(), Arc::new(AcceptingMarker));
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    let epoch = admin
        .bump_epoch(Timestamp::from_unix_micros(START.as_unix_micros()))
        .await
        .expect("bump");

    // Assert
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let bumps: Vec<_> = logged
        .iter()
        .filter(|line| line.contains("epoch bumped"))
        .collect();
    assert_eq!(bumps.len(), 1, "{logged:?}");
    assert!(bumps[0].contains("\"level\":\"info\""), "{}", bumps[0]);
    assert!(epoch > 7);
    drop((admin, api));
    assert_eq!(db.finish().await, 0, "connections left open");
}
