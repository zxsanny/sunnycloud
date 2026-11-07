//! The log lines of the scheduled scrub: INFO at the start and at the end of a cycle, WARN when the activity
//! gauge keeps the scrub paused until the cycle budget ends. This binary holds this one test: the capturing
//! subscriber is installed on the test's thread, and a scoped default races with the callsite interest cache when
//! other tests run in parallel.

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sc_clock::Clock;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sunnycloud_blobstore::ScrubJanitor;
use support::Rig;
use tracing::Level;

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

fn count(lines: &[String], level: &str, text: &str) -> usize {
    lines
        .iter()
        .filter(|line| line.contains(&format!("\"level\":\"{level}\"")) && line.contains(text))
        .count()
}

#[tokio::test]
async fn a_cycle_logs_its_start_and_end_and_a_pause_that_outlasts_the_budget_is_a_warning() {
    // Arrange
    let rig = Rig::new().await;
    rig.store(64 * 1024, 1).await;
    let mut config = rig.config.clone();
    config.scrub_cycle_budget = Duration::from_millis(200);
    let scrub = rig.scrub_with(config.clone(), 10, Some(rig.gauge.clone()));
    let janitor = ScrubJanitor::new(&config, scrub.scrubber.clone());
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        rig.clock.clone(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);
    let busy = (rig.gauge.enter(), rig.gauge.enter());

    // Act
    janitor
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("paused cycle");
    let paused = lines.all();
    drop(busy);
    janitor
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("free cycle");

    // Assert
    let logged = lines.all();
    assert_eq!(count(&paused, "warn", "scrub paused"), 1, "{paused:?}");
    assert_eq!(count(&logged, "warn", "scrub paused"), 1, "{logged:?}");
    assert_eq!(
        count(&logged, "info", "scrub cycle started"),
        2,
        "{logged:?}"
    );
    assert_eq!(
        count(&logged, "info", "scrub cycle finished"),
        2,
        "{logged:?}"
    );
    assert!(
        logged
            .iter()
            .filter(|line| line.contains("scrub cycle finished"))
            .all(|line| line.contains("duration_ms"))
    );
    assert_eq!(count(&logged, "error", ""), 0, "{logged:?}");
    drop((janitor, scrub));
    rig.finish().await;
}
