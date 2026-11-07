//! A missing feed row is an invariant violation, logged at ERROR with the account id. This binary holds this
//! one test: the capturing subscriber is installed on the test's thread, and a scoped default races with the
//! callsite interest cache when other tests run in parallel.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::FeedStampError;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sc_testdb::clock_at_start;
use sunnycloud_sync_feed::account_feed_missing;
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

#[test]
fn missing_feed_row_logs_one_error_line_with_the_account_id() {
    // Arrange
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
    let account = Uuid::new_v4();

    // Act
    let error = account_feed_missing(account);

    // Assert
    assert!(matches!(error, FeedStampError::AccountFeedMissing));
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(logged[0].contains("\"level\":\"error\""), "{}", logged[0]);
    assert!(logged[0].contains(&account.to_string()), "{}", logged[0]);
}
