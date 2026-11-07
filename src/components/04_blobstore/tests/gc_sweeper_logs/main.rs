//! The log lines of the sweeper (AC-4: WARN for a file that is already missing; INFO at the start and at the
//! summary of a cycle). This binary holds this one test: the capturing subscriber is installed on the test's
//! thread, and a scoped default races with the callsite interest cache when other tests run in parallel.

#[allow(dead_code)]
#[path = "../rows/support.rs"]
mod rows;
#[allow(dead_code)]
#[path = "../gc_sweeper/support.rs"]
mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::Due;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use support::{Scene, after, gc_config};
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

#[tokio::test]
async fn a_missing_file_is_a_warning_and_its_rows_are_still_removed() {
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(3, Due::Now).await;
    let missing = ids[1];
    std::fs::remove_file(scene.file(missing)).expect("remove the file");
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        sc_testdb::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);
    let sweeper = scene.sweeper(
        &gc_config(
            &scene.rig,
            10,
            Duration::from_secs(60),
            Duration::from_secs(60),
        ),
        None,
    );

    // Act
    let stats = Scene::run(&sweeper, after(rows::START, Duration::from_secs(1))).await;

    // Assert
    assert_eq!(stats.removed, 3);
    assert_eq!(scene.rows().await, 0);
    assert_eq!(scene.cleanup.calls(missing), Some((1, true)));
    let logged = lines.all();
    let warnings: Vec<_> = logged
        .iter()
        .filter(|line| line.contains("\"level\":\"warn\""))
        .collect();
    assert_eq!(warnings.len(), 1, "{logged:?}");
    assert!(
        warnings[0].contains(&missing.to_string()),
        "{}",
        warnings[0]
    );
    assert!(
        logged
            .iter()
            .any(|line| line.contains("gc cycle started") && line.contains("\"level\":\"info\"")),
        "{logged:?}"
    );
    assert!(
        logged.iter().any(|line| line.contains("gc cycle finished")
            && line.contains("\"level\":\"info\"")
            && line.contains("duration_ms")),
        "{logged:?}"
    );
    assert!(
        !logged
            .iter()
            .any(|line| line.contains("\"level\":\"error\""))
    );
    drop(sweeper);
    scene.finish().await;
}
