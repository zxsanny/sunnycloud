//! AC-5: the janitor families on `/metrics` and the `scheduler_alive` item on `/readyz`, through the real ops
//! router.

mod janitor_support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use janitor_support::{Counting, Erroring, get, metrics, series, system_clock, wait_until};
use sc_clock::{ManualClock, Timestamp};

use sc_observability::{Liveness, ReadinessRegistry, ReadinessStatus, ops_router};
use serde_json::Value;
use sunnycloud_server::janitor::{JanitorMetrics, Scheduled, Scheduler};

const START: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);
const INTERVAL: Duration = Duration::from_millis(10);

#[tokio::test]
async fn metrics_exist_per_janitor_and_scheduler_alive_turns_failing_after_stop() {
    // Arrange
    let ok = Counting::new("ok_janitor");
    let failing = Erroring::new("failing_janitor");
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));
    let scheduler = Scheduler::start(
        system_clock(),
        janitor_metrics,
        vec![
            Scheduled {
                janitor: ok.clone(),
                interval: INTERVAL,
            },
            Scheduled {
                janitor: failing.clone(),
                interval: INTERVAL,
            },
        ],
    )
    .expect("scheduler starts");
    let mut registry = ReadinessRegistry::new();
    registry.add(scheduler.readiness());
    let router = ops_router(metrics, registry, Liveness::new());
    wait_until("both janitors to cycle", || {
        ok.calls() >= 2 && failing.calls() >= 2
    })
    .await;

    // Act
    let (ready_status, ready_body) = get(&router, "/readyz").await;
    let (_, scrape) = get(&router, "/metrics").await;
    scheduler.stop().await;
    let calls_at_stop = (ok.calls(), failing.calls());
    let (stopped_status, stopped_body) = get(&router, "/readyz").await;
    tokio::time::sleep(INTERVAL * 5).await;

    // Assert
    let item = |body: &str| {
        let body: Value = serde_json::from_str(body).expect("readyz body");
        body["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["name"] == "scheduler_alive")
            .expect("scheduler_alive is listed")
            .clone()
    };
    assert_eq!(ready_status, StatusCode::OK);
    assert_eq!(item(&ready_body)["status"], "ok");
    assert_eq!(stopped_status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(item(&stopped_body)["status"], "failing");
    let cycles = "sunnycloud_janitor_cycles_total";
    assert!(series(&scrape, cycles, &[("task", "ok_janitor"), ("result", "ok")]) >= Some(2.0));
    assert!(
        series(
            &scrape,
            cycles,
            &[("task", "failing_janitor"), ("result", "failed")]
        ) >= Some(2.0)
    );
    let last_success = "sunnycloud_janitor_last_success_timestamp_seconds";
    assert!(series(&scrape, last_success, &[("task", "ok_janitor")]).is_some());
    assert_eq!(
        series(&scrape, last_success, &[("task", "failing_janitor")]),
        None
    );
    assert_eq!((ok.calls(), failing.calls()), calls_at_stop);
}

#[tokio::test]
async fn last_success_is_the_clock_time_of_the_finished_cycle() {
    // Arrange
    let clock = Arc::new(ManualClock::new(START));
    let ok = Counting::new("ok_janitor");
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));
    let scheduler = Scheduler::start(
        clock.clone(),
        janitor_metrics,
        vec![Scheduled {
            janitor: ok.clone(),
            interval: Duration::from_secs(1),
        }],
    )
    .expect("scheduler starts");

    // Act
    clock.tick(Duration::from_secs(1));
    wait_until("the cycle", || ok.calls() == 1).await;
    scheduler.stop().await;
    let text = metrics.render().await.expect("metrics render");

    // Assert
    let seconds = series(
        &text,
        "sunnycloud_janitor_last_success_timestamp_seconds",
        &[("task", "ok_janitor")],
    );
    assert_eq!(seconds, Some(1_794_052_801.0));
}

#[tokio::test]
async fn scheduler_alive_fails_when_the_clock_shuts_down_the_loops() {
    // Arrange
    let clock = Arc::new(ManualClock::new(START));
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));
    let scheduler = Scheduler::start(
        clock.clone(),
        janitor_metrics,
        vec![Scheduled {
            janitor: Counting::new("ok_janitor"),
            interval: Duration::from_secs(1),
        }],
    )
    .expect("scheduler starts");
    let alive = scheduler.readiness();
    let before = alive.check().await;

    // Act
    clock.shutdown();
    let after = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let item = alive.check().await;
            if item.status != ReadinessStatus::Ok {
                return item;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("the loop ends");

    // Assert
    assert_eq!(before.status, ReadinessStatus::Ok);
    assert_eq!(after.status, ReadinessStatus::Failing);
    assert_eq!(alive.name(), "scheduler_alive");
    scheduler.stop().await;
}
