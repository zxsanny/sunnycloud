//! AZ-3820 AC-6: a deletion older than `account_delete_alert_after` (20 s in test) that is not finished is
//! logged at WARN with the account id; at the finalize `account_deletion_seconds` records the time from the
//! request to the finalize. The manual clock moves only by the test and by the hook double.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{AccountLifecycleHook, EraseProgress, Janitor, LifecycleError, async_trait};
use sc_clock::{Clock, ManualClock};
use sc_db::DbTx;
use sc_observability::{BuildInfo, Metrics};
use sc_testdb::START;
use sqlx::Connection;
use sunnycloud_identity::IdentityAccounts;
use uuid::Uuid;

use crate::capture;
use crate::purge_support::probes::state;
use crate::purge_support::world::PurgeWorld;
use crate::support::seed;

/// Reports 1 left for its first `busy` calls, then 0; each call costs `cost` on the clock.
struct Ticking {
    clock: Arc<ManualClock>,
    busy: Mutex<u32>,
    cost: Duration,
}

#[async_trait]
impl AccountLifecycleHook for Ticking {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Err(LifecycleError::Unexpected(
            "not called by the purger".into(),
        ))
    }

    async fn erase_batch(
        &self,
        _account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        self.clock.tick(self.cost);
        let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        let remaining = u64::from(*busy > 0);
        *busy = busy.saturating_sub(1);
        Ok(EraseProgress { remaining })
    }
}

fn metrics() -> Metrics {
    Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics")
}

/// `(sum, count)` of `account_deletion_seconds` in the text format.
async fn deletion_seconds(metrics: &Metrics) -> (f64, u64) {
    let text = metrics.render().await.expect("render");
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .map(|rest| rest.trim().to_owned())
            .unwrap_or_else(|| panic!("{name} missing in\n{text}"))
    };
    (
        value("sunnycloud_account_deletion_seconds_sum ")
            .parse()
            .expect("sum"),
        value("sunnycloud_account_deletion_seconds_count ")
            .parse()
            .expect("count"),
    )
}

struct Alerting {
    world: PurgeWorld,
    clock: Arc<ManualClock>,
    account: Uuid,
}

/// A user whose deletion was requested at the start time through 07's service and 01.
async fn requested() -> Alerting {
    capture::install();
    let clock = sc_testdb::clock_at_start();
    let world = PurgeWorld::new(clock.clone()).await;
    let account = Uuid::new_v4();
    let mut conn = world.fixture().await;
    seed::account(
        &mut conn,
        account,
        "Alert Fixture",
        "user",
        "active",
        START.as_unix_micros(),
    )
    .await;
    conn.close().await.expect("close");
    world.request_deletion(account).await;
    Alerting {
        world,
        clock,
        account,
    }
}

impl Alerting {
    fn hook(&self, busy: u32) -> Vec<Arc<dyn AccountLifecycleHook>> {
        let budget = self.world.config.account_delete_cycle_budget;
        vec![Arc::new(Ticking {
            clock: self.clock.clone(),
            busy: Mutex::new(busy),
            cost: budget / 3,
        })]
    }

    fn purger(&self, busy: u32, metrics: &Metrics) -> sunnycloud_admin::DeletionPurger {
        let clock: Arc<dyn Clock> = self.clock.clone();
        let accounts = Arc::new(IdentityAccounts::new(self.world.api.clone(), clock));
        self.world
            .purger(accounts, self.hook(busy), self.world.api.clone())
            .with_metrics(metrics)
            .expect("metric registered")
    }
}

#[tokio::test]
async fn an_unfinished_deletion_past_the_alert_time_warns_and_the_finalize_records_its_duration() {
    // Arrange
    let alerting = requested().await;
    let metrics = metrics();
    let purger = alerting.purger(3, &metrics);
    let alert_after = alerting.world.config.account_delete_alert_after;
    alerting.clock.tick(alert_after + Duration::from_secs(1));

    // Act
    let first = purger
        .run_cycle(alerting.clock.now_utc())
        .await
        .expect("first cycle");
    let warns_after_first = capture::at_level(&capture::of_account(alerting.account), "warn")
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let second = purger
        .run_cycle(alerting.clock.now_utc())
        .await
        .expect("second cycle");

    // Assert
    assert_eq!(first.processed, 0, "the first cycle ends at the budget");
    assert_eq!(warns_after_first.len(), 1, "{warns_after_first:#?}");
    assert!(warns_after_first[0].contains("account_delete_alert_after"));
    assert_eq!(second.processed, 1);
    let (sum, count) = deletion_seconds(&metrics).await;
    assert_eq!(count, 1);
    // 21 s before the first cycle, three budget thirds in it, two more for the two passes of 0 before the finalize.
    let expected = alert_after.as_secs_f64() + 1.0 + 0.5;
    assert!((sum - expected).abs() < 1e-6, "{sum} != {expected}");
    let mut conn = alerting.world.fixture().await;
    assert_eq!(state(&mut conn, alerting.account).await, None);
    conn.close().await.expect("close");
    alerting.world.finish().await;
}

#[tokio::test]
async fn a_deletion_younger_than_the_alert_time_logs_no_warn() {
    // Arrange
    let alerting = requested().await;
    let metrics = metrics();
    let purger = alerting.purger(0, &metrics);

    // Act
    let stats = purger
        .run_cycle(alerting.clock.now_utc())
        .await
        .expect("cycle");

    // Assert
    assert_eq!(stats.processed, 1);
    let lines = capture::of_account(alerting.account);
    assert_eq!(capture::at_level(&lines, "warn").len(), 0, "{lines:#?}");
    let (sum, count) = deletion_seconds(&metrics).await;
    assert_eq!(count, 1);
    assert!((sum - 0.2).abs() < 1e-6, "two passes of 0: {sum}");
    alerting.world.finish().await;
}
