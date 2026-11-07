//! AC-3, one statement: `views` for 500 accounts runs one unit of work and one statement. The server has no
//! `pg_stat_statements`, so the statements are counted from the statement events of sqlx, captured at DEBUG
//! (the log line drops the statement text, so only the count is usable): a system-scope unit of work with a
//! single `SELECT 1` gives the number of events one statement costs, and `views` must cost the same.
//! This binary holds this one test: the capturing subscriber is installed on the test's thread.

mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use support::{DEFAULT_QUOTA, Seeded, admin, seeded};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

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
async fn views_of_500_ids_is_one_unit_of_work_with_the_statement_count_of_a_single_select() {
    // Arrange
    let Seeded { harness, expected } = seeded().await;
    let quota = admin(&harness.api, DEFAULT_QUOTA);
    let mut ids: Vec<Uuid> = expected.keys().copied().collect();
    ids.extend([Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()]);
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::DEBUG,
            component: Component::Api,
        },
        support::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);
    harness
        .api
        .run_system("quota_statement_baseline", async |tx| {
            sqlx::query("SELECT 1").execute(tx.conn()).await
        })
        .await
        .expect("baseline");
    let one_statement = lines.take().len();
    let units_before = unit_count(&harness);

    // Act
    let views = quota.views(&ids).await.unwrap();

    // Assert
    let events = lines.take().len();
    assert_eq!(views, expected);
    assert_eq!(unit_count(&harness) - units_before, 1);
    assert!(one_statement > 0);
    assert_eq!(events, one_statement);
    harness.finish().await;
}

fn unit_count(harness: &support::Harness) -> u64 {
    harness
        .api
        .stats()
        .queries
        .iter()
        .map(|query| query.count)
        .sum()
}
