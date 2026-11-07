//! A missing feed row is an invariant violation, logged at ERROR with the account id. This binary holds this
//! one test: the capturing subscriber is installed on the test's thread, and a scoped default races with the
//! callsite interest cache when other tests run in parallel.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{FeedStamp, FeedStampError, TouchedNodes};
use sc_db::{RunError, Scope};
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sc_testdb::{MIGRATOR, START, TestDb, clock_at_start};
use sqlx::Connection;
use sunnycloud_sync_feed::PgFeedStamp;
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

#[tokio::test]
async fn missing_feed_row_logs_one_error_line_with_the_account_id() {
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
    conn.close().await.expect("close owner connection");
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::WARN,
            component: Component::Api,
        },
        clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    let result = api
        .run(Scope::Account(account), async |tx| {
            PgFeedStamp
                .stamp(tx, account, TouchedNodes(vec![Uuid::new_v4()]))
                .await
        })
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(RunError::Closure(FeedStampError::AccountFeedMissing))
    ));
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(logged[0].contains("\"level\":\"error\""), "{}", logged[0]);
    assert!(logged[0].contains(&account.to_string()), "{}", logged[0]);
    drop(api);
    assert_eq!(db.finish().await, 0, "connections left open");
}
