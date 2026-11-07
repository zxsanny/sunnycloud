//! AC-1, log: a recompute that finds drift logs one WARN line with the account id and no counter value; one that
//! finds none logs nothing. This binary holds this one test: the capturing subscriber is installed on the test's
//! thread, and a scoped default races with the callsite interest cache when other tests run in parallel.

mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use support::{GIB, Harness, MIB, admin, initialized};
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
async fn drift_logs_one_warn_with_ids_only_and_no_drift_logs_nothing() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    let live = 600 * MIB;
    initialized(&harness, GIB, id, None, live + 777, 35 * MIB).await;
    let quota = admin(&harness.api, GIB);
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        support::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    quota
        .recompute_columns(id, live, 40 * MIB, false)
        .await
        .unwrap();
    let after_report = lines.take();
    quota
        .recompute_columns(id, live, 40 * MIB, true)
        .await
        .unwrap();
    let after_repair = lines.take();
    quota
        .recompute_columns(id, live, 40 * MIB, false)
        .await
        .unwrap();
    let after_recheck = lines.take();

    // Assert
    for logged in [&after_report, &after_repair] {
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(logged[0].contains("\"level\":\"warn\""), "{}", logged[0]);
        assert!(logged[0].contains(&id.to_string()), "{}", logged[0]);
        for value in [live + 777, 35 * MIB, live, 40 * MIB] {
            assert!(!logged[0].contains(&value.to_string()), "{}", logged[0]);
        }
    }
    assert!(after_recheck.is_empty(), "{after_recheck:?}");
    harness.finish().await;
}
