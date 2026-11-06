//! AC-6: a release that would go negative clamps at zero, is not an error and logs exactly one WARN line with
//! the account id. This binary holds this one test: the capturing subscriber is installed on the test's thread,
//! and a scoped default races with the callsite interest cache when other tests run in parallel.

mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::QuotaLedger;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sqlx::Connection;
use support::{Harness, MIB, account_with_quota, in_tx, ledger, quota_row};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
async fn release_clamps_at_zero_and_logs_one_warn_with_the_account_id() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(1 << 30), 0, 100 * MIB).await;
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::WARN,
            component: Component::Api,
        },
        support::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);
    let ledger = ledger(1 << 30);

    // Act
    let within = in_tx(&harness.api, account, async |tx| {
        ledger.release_reservation(tx, account, 40 * MIB).await
    })
    .await;
    let after_within = (lines.all().len(), quota_row(&mut conn, account).await.2);
    let beyond = in_tx(&harness.api, account, async |tx| {
        ledger.release_reservation(tx, account, 200 * MIB).await
    })
    .await;

    // Assert
    within.unwrap();
    beyond.unwrap();
    assert_eq!(after_within, (0, 60 * MIB));
    assert_eq!(quota_row(&mut conn, account).await.2, 0);
    let logged = lines.all();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(logged[0].contains("\"level\":\"warn\""), "{}", logged[0]);
    assert!(logged[0].contains(&account.to_string()), "{}", logged[0]);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}
