//! The log lines of the orphan sweeper (AC-2: one WARN per orphan, ids only; AC-3: an ERROR line when the run
//! aborts; INFO at the start and at the summary of a cycle). This binary holds this one test: the capturing
//! subscriber is installed on the test's thread, and a scoped default races with the callsite interest cache when
//! other tests run in parallel.

#[allow(dead_code)]
#[path = "../rows/support.rs"]
mod rows;
#[allow(dead_code)]
#[path = "../orphan_sweeper/support.rs"]
mod support;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use support::{AGE, Scene, Volume, blob_id, make_blocker};
use tracing::Level;
use uuid::Uuid;

use rows::START;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn with(&self, level: &str, message: &str) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|line| {
                line.contains(&format!("\"level\":\"{level}\"")) && line.contains(message)
            })
            .collect()
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

fn names_exactly(lines: &[String], ids: &[Uuid]) -> bool {
    let named: HashSet<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| lines.iter().any(|line| line.contains(&id.to_string())))
        .collect();
    lines.len() == ids.len() && named.len() == ids.len()
}

#[tokio::test]
async fn orphans_are_warned_one_by_one_and_an_aborted_run_is_an_error() {
    // Arrange
    let scene = Scene::new().await;
    let volume = Volume::build(&scene, START).await;
    let parked = {
        let id = blob_id(5, 900);
        scene.file_aged(id, START, 10 * AGE);
        let mut name = scene.path(id).into_os_string();
        name.push(".gc");
        fs::rename(scene.path(id), &name).expect("park the file");
        id
    };
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
    let report = scene.sweeper("report", AGE);
    let delete = scene.sweeper("delete", AGE);

    // Act
    let reported = Scene::run(&report, START).await;
    let after_report = lines.all().len();
    let blocker = make_blocker(&scene, 0x0100);
    let aborted = delete.run_cycle(START).await;
    let after_abort = lines.all().len();
    fs::remove_file(&blocker).expect("remove the blocker");
    let deleted = Scene::run(&delete, START).await;

    // Assert
    let warnings = lines.with("warn", "orphan blob file");
    assert_eq!((reported.found, reported.removed), (20, 0));
    assert_eq!(warnings.len(), 40, "{warnings:?}");
    assert!(names_exactly(&warnings[..20], &volume.old), "{warnings:?}");
    assert!(
        !lines
            .all()
            .iter()
            .any(|line| line.contains(&parked.to_string())),
        "the parked file is the GC sweeper's"
    );
    assert!(aborted.is_err());
    let abort_lines: Vec<String> = lines.all()[after_report..after_abort].to_vec();
    assert_eq!(abort_lines.len(), 2, "{abort_lines:?}");
    assert!(abort_lines[0].contains("orphan cycle started"));
    assert!(abort_lines[1].contains("\"level\":\"error\""));
    assert!(abort_lines[1].contains("orphan sweep aborted"));
    assert_eq!((deleted.found, deleted.removed), (20, 20));
    assert!(names_exactly(&warnings[20..], &volume.old), "{warnings:?}");
    assert_eq!(lines.with("info", "orphan cycle started").len(), 3);
    let finished = lines.with("info", "orphan cycle finished");
    assert_eq!(finished.len(), 2);
    assert!(finished.iter().all(|line| line.contains("duration_ms")));
    assert_eq!(lines.with("error", "").len(), 1);
    drop((report, delete));
    scene.finish().await;
}
