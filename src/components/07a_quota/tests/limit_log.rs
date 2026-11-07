//! AC-2, log: each accepted limit change logs one INFO line with the account id and nothing else about the
//! account; a refused change logs nothing. This binary holds this one test: the capturing subscriber is
//! installed on the test's thread, and a scoped default races with the callsite interest cache when other
//! tests run in parallel.

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
async fn each_accepted_change_logs_one_info_line_with_ids_only_and_refusals_log_nothing() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    initialized(&harness, 10 * GIB, id, None, 600 * MIB, 0).await;
    let quota = admin(&harness.api, 10 * GIB);
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
    quota.set_limit(id, Some(500 * MIB)).await.unwrap();
    let after_lowering = lines.take();
    quota.set_limit(id, None).await.unwrap();
    let after_default = lines.take();
    quota.set_limit(id, Some(0)).await.unwrap();
    let after_zero = lines.take();
    let refused = [
        quota.set_limit(id, Some(-1)).await,
        quota.set_limit(id, Some((1 << 53) + 1)).await,
        quota.set_limit(Uuid::new_v4(), Some(GIB)).await,
    ];
    let after_refusals = lines.take();

    // Assert
    for logged in [&after_lowering, &after_default, &after_zero] {
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(logged[0].contains("\"level\":\"info\""), "{}", logged[0]);
        assert!(logged[0].contains(&id.to_string()), "{}", logged[0]);
    }
    assert!(!after_lowering[0].contains(&(500 * MIB).to_string()));
    assert!(refused.iter().all(Result::is_err));
    assert!(after_refusals.is_empty(), "{after_refusals:?}");
    harness.finish().await;
}
