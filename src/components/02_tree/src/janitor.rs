//! The cascade and the tombstone purge as `Janitor`s for 12's scheduler. A cycle runs batches until one handles
//! nothing or `tree_maintenance_interval` has passed on the injected clock, so one tick never runs into the next.
//! Logs carry no names or paths: a cascade batch slower than the interval is a WARN, a batch that handled rows
//! an INFO with its duration.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    CycleStats, Janitor, JanitorError, MaintenanceError, TreeMaintenance, async_trait,
};
use sc_clock::{Clock, Timestamp};
use sc_observability::{log_info, log_warn};

use crate::config::TreeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Cascade,
    Purge,
}

pub struct TreeJanitor {
    maintenance: Arc<dyn TreeMaintenance>,
    clock: Arc<dyn Clock>,
    job: Job,
    batch: u32,
    budget: Duration,
}

impl TreeJanitor {
    /// Batches of `cascade_batch_size`.
    pub fn cascade(
        maintenance: Arc<dyn TreeMaintenance>,
        config: &TreeConfig,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::new(
            maintenance,
            clock,
            Job::Cascade,
            config.cascade_batch_size,
            config,
        )
    }

    /// Batches of `purge_batch_size`.
    pub fn purge(
        maintenance: Arc<dyn TreeMaintenance>,
        config: &TreeConfig,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::new(
            maintenance,
            clock,
            Job::Purge,
            config.purge_batch_size,
            config,
        )
    }

    fn new(
        maintenance: Arc<dyn TreeMaintenance>,
        clock: Arc<dyn Clock>,
        job: Job,
        batch: u64,
        config: &TreeConfig,
    ) -> Self {
        TreeJanitor {
            maintenance,
            clock,
            job,
            batch: u32::try_from(batch).unwrap_or(u32::MAX),
            budget: config.tree_maintenance_interval,
        }
    }

    async fn batch(&self, now: Timestamp) -> Result<u64, MaintenanceError> {
        match self.job {
            Job::Cascade => self.maintenance.cascade_batch(now, self.batch).await,
            Job::Purge => self.maintenance.purge_tombstones(now, self.batch).await,
        }
    }

    // The log schema has no count field, so the INFO line carries the batch's duration only.
    fn log_batch(&self, handled: u64, took: Duration) {
        let duration_ms = u64::try_from(took.as_millis()).unwrap_or(u64::MAX);
        if self.job == Job::Cascade && took > self.budget {
            log_warn!(
                "tree cascade batch slower than the maintenance interval",
                duration_ms = duration_ms
            );
        }
        if handled > 0 {
            match self.job {
                Job::Cascade => log_info!("tree cascade batch finished", duration_ms = duration_ms),
                Job::Purge => log_info!("tree purge batch finished", duration_ms = duration_ms),
            }
        }
    }
}

#[async_trait]
impl Janitor for TreeJanitor {
    fn name(&self) -> &'static str {
        match self.job {
            Job::Cascade => "tree_cascade",
            Job::Purge => "tree_purge",
        }
    }

    /// Reports the rows handled over the cycle; a cycle with nothing due runs one empty batch and reports 0.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let started = self.clock.monotonic();
        let mut processed = 0;
        loop {
            let batch_started = self.clock.monotonic();
            let handled = self
                .batch(now)
                .await
                .map_err(|MaintenanceError::Unexpected(fault)| JanitorError::Unexpected(fault))?;
            let took = self
                .clock
                .monotonic()
                .saturating_duration_since(batch_started);
            self.log_batch(handled, took);
            processed += handled;
            let spent = self.clock.monotonic().saturating_duration_since(started);
            if handled == 0 || spent >= self.budget {
                break;
            }
        }
        Ok(CycleStats { processed })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sc_clock::ManualClock;

    use super::*;

    const START: Timestamp = Timestamp::from_unix_micros(1_800_000_000_000_000);

    /// Hands out `due` rows in batches of at most `limit`, advancing the clock by `cost` per batch.
    struct Scripted {
        clock: Arc<ManualClock>,
        cost: Duration,
        due: Mutex<u64>,
        calls: Mutex<Vec<(&'static str, u32)>>,
    }

    impl Scripted {
        fn take(&self, job: &'static str, limit: u32) -> u64 {
            self.clock.tick(self.cost);
            self.calls.lock().expect("calls").push((job, limit));
            let mut due = self.due.lock().expect("due");
            let handled = (*due).min(u64::from(limit));
            *due -= handled;
            handled
        }
    }

    #[async_trait]
    impl TreeMaintenance for Scripted {
        async fn cascade_batch(&self, _: Timestamp, limit: u32) -> Result<u64, MaintenanceError> {
            Ok(self.take("cascade", limit))
        }

        async fn purge_tombstones(
            &self,
            _: Timestamp,
            limit: u32,
        ) -> Result<u64, MaintenanceError> {
            Ok(self.take("purge", limit))
        }
    }

    fn config() -> TreeConfig {
        TreeConfig {
            tombstone_horizon: Duration::from_secs(2),
            listing_default_limit: 1000,
            listing_max_limit: 1000,
            cascade_batch_size: 20,
            purge_batch_size: 10,
            tree_maintenance_interval: Duration::from_secs(30),
            conflict_copy_time_zone: "UTC".to_owned(),
        }
    }

    fn scripted(due: u64, cost: Duration) -> (Arc<Scripted>, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new(START));
        let maintenance = Arc::new(Scripted {
            clock: clock.clone(),
            cost,
            due: Mutex::new(due),
            calls: Mutex::new(Vec::new()),
        });
        (maintenance, clock)
    }

    #[tokio::test]
    async fn a_cycle_stops_at_the_time_budget_with_work_still_due() {
        // Arrange
        let (maintenance, clock) = scripted(1000, Duration::from_secs(10));
        let janitor = TreeJanitor::cascade(maintenance.clone(), &config(), clock);

        // Act
        let stats = janitor.run_cycle(START).await.expect("cycle");

        // Assert
        assert_eq!(
            stats.processed, 60,
            "three batches of 20 fill the 30 s budget"
        );
        assert_eq!(
            *maintenance.calls.lock().expect("calls"),
            [("cascade", 20); 3]
        );
        assert_eq!(*maintenance.due.lock().expect("due"), 940);
    }

    #[tokio::test]
    async fn a_cycle_drains_inside_the_budget_and_stops_on_an_empty_batch() {
        // Arrange
        let (maintenance, clock) = scripted(25, Duration::from_secs(1));
        let janitor = TreeJanitor::purge(maintenance.clone(), &config(), clock);

        // Act
        let stats = janitor.run_cycle(START).await.expect("cycle");

        // Assert
        assert_eq!(stats.processed, 25);
        assert_eq!(
            *maintenance.calls.lock().expect("calls"),
            [("purge", 10); 4],
            "10, 10, 5, then the empty batch"
        );
    }

    #[tokio::test]
    async fn an_idle_cycle_runs_one_batch_and_reports_zero() {
        // Arrange
        let (maintenance, clock) = scripted(0, Duration::from_secs(1));
        let janitor = TreeJanitor::cascade(maintenance.clone(), &config(), clock);

        // Act
        let stats = janitor.run_cycle(START).await.expect("cycle");

        // Assert
        assert_eq!(stats, CycleStats { processed: 0 });
        assert_eq!(maintenance.calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn the_janitors_are_named_by_their_job() {
        // Arrange
        let (maintenance, clock) = scripted(0, Duration::ZERO);

        // Act
        let names = [
            TreeJanitor::cascade(maintenance.clone(), &config(), clock.clone()).name(),
            TreeJanitor::purge(maintenance, &config(), clock).name(),
        ];

        // Assert
        assert_eq!(names, ["tree_cascade", "tree_purge"]);
    }
}
