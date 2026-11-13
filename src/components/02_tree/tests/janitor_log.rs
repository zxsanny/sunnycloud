//! AZ-3714 logs: a cascade batch slower than `tree_maintenance_interval` on the injected clock logs one WARN with
//! its duration, a batch that handled rows one INFO, and no line carries a name or a path. This binary holds this
//! one test: the capturing subscriber is installed on the test's thread, and a scoped default races with the
//! callsite interest cache when other tests run in parallel.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{Janitor, MaintenanceError, TreeMaintenance, async_trait};
use sc_clock::{ManualClock, Timestamp};
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sunnycloud_tree::{TreeConfig, TreeJanitor};
use tracing::Level;

const START: Timestamp = Timestamp::from_unix_micros(1_800_000_000_000_000);

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

/// Each cascade batch takes `costs[i]` on the clock and marks 7 rows; past the list it marks nothing.
struct Timed {
    clock: Arc<ManualClock>,
    costs: Mutex<Vec<Duration>>,
}

#[async_trait]
impl TreeMaintenance for Timed {
    async fn cascade_batch(&self, _: Timestamp, _: u32) -> Result<u64, MaintenanceError> {
        let mut costs = self.costs.lock().unwrap_or_else(PoisonError::into_inner);
        if costs.is_empty() {
            return Ok(0);
        }
        self.clock.tick(costs.remove(0));
        Ok(7)
    }

    async fn purge_tombstones(&self, _: Timestamp, _: u32) -> Result<u64, MaintenanceError> {
        Ok(0)
    }
}

#[tokio::test]
async fn a_cascade_batch_slower_than_the_interval_logs_one_warn_and_each_batch_one_info() {
    // Arrange
    let clock = Arc::new(ManualClock::new(START));
    let config = TreeConfig {
        tombstone_horizon: Duration::from_secs(2),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 20,
        purge_batch_size: 10,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    };
    let maintenance = Arc::new(Timed {
        clock: clock.clone(),
        costs: Mutex::new(vec![Duration::from_secs(1), Duration::from_secs(31)]),
    });
    let janitor = TreeJanitor::cascade(maintenance, &config, clock.clone());
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        clock,
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    let stats = janitor.run_cycle(START).await.expect("cycle");

    // Assert
    assert_eq!(stats.processed, 14, "the slow batch ends the cycle");
    let logged = lines.all();
    let warns: Vec<&String> = logged
        .iter()
        .filter(|line| line.contains("\"level\":\"warn\""))
        .collect();
    assert_eq!(warns.len(), 1, "{logged:?}");
    assert!(
        warns[0].contains("slower than the maintenance interval"),
        "{}",
        warns[0]
    );
    assert!(
        warns[0].contains("31000"),
        "the batch's duration: {}",
        warns[0]
    );
    let infos = logged
        .iter()
        .filter(|line| line.contains("tree cascade batch finished"))
        .count();
    assert_eq!(infos, 2, "{logged:?}");
}
