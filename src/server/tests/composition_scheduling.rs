//! AZ-3789 and AZ-3792 in the composition root: the janitors on their owners' intervals, the one scheduler, and
//! the readiness list (the database, then `scheduler_alive`). Split from `composition_root` by responsibility.

mod janitor_support;
mod root_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use janitor_support::{get, series};
use root_support::{INTERVALS, Root};
use sc_config::parse_duration;
use sc_observability::{
    Liveness, Metrics, ReadinessCheck as _, ReadinessRegistry, ReadinessStatus, ops_router,
};
use sunnycloud_server::janitor::Scheduler;
use sunnycloud_server::root::{self, DatabaseReady, Janitors, ReadinessChecks};

fn interval(key: &str) -> Duration {
    let (_, text) = INTERVALS
        .iter()
        .find(|(name, _)| *name == key)
        .unwrap_or_else(|| panic!("{key} has no test interval"));
    parse_duration(text).expect("a duration")
}

#[tokio::test]
async fn janitors_run_on_their_owners_configured_intervals() {
    // Arrange
    let root = Root::new().await;

    // Act
    let janitors = root.services.get::<Janitors>().await.expect("janitors");

    // Assert
    let actual: Vec<_> = janitors
        .0
        .iter()
        .map(|job| (job.janitor.name(), job.interval))
        .collect();
    let expected = [
        ("session_reaper", interval("session_reap_interval")),
        ("receipt_janitor", interval("receipt_janitor_interval")),
        ("progress_flush", interval("checkpoint_interval")),
        ("gc_sweeper", interval("gc_interval")),
        ("orphan_sweeper", interval("orphan_sweep_interval")),
        ("scrub_janitor", interval("scrub_interval")),
        ("tree_purge", interval("tree_maintenance_interval")),
        ("tree_cascade", interval("tree_maintenance_interval")),
        ("deletion_purger", interval("account_delete_interval")),
        ("identity", interval("identity_janitor_interval")),
        ("job_reaper", interval("job_reap_interval")),
        ("derived_janitor", interval("derived_janitor_interval")),
        ("audit_retention", interval("audit_retention_interval")),
    ];
    assert_eq!(actual, expected);
    root.finish().await;
}

async fn statuses(checks: &ReadinessChecks) -> Vec<(String, ReadinessStatus)> {
    let mut items = Vec::new();
    for check in &checks.0 {
        items.push((check.name().to_owned(), check.check().await.status));
    }
    items
}

#[tokio::test]
async fn readiness_list_is_the_database_then_scheduler_alive_and_both_are_ok_once_started() {
    // Arrange
    let root = Root::new().await;

    // Act
    let checks = root
        .services
        .get::<ReadinessChecks>()
        .await
        .expect("readiness");
    let built = statuses(&checks).await;
    root::start_scheduler(&root.services)
        .await
        .expect("scheduler starts");
    let started = statuses(&checks).await;

    // Assert
    assert_eq!(
        built,
        [
            ("database".to_owned(), ReadinessStatus::Ok),
            ("scheduler_alive".to_owned(), ReadinessStatus::Failing),
        ],
        "the scheduler is not started by building the list"
    );
    assert_eq!(
        started,
        [
            ("database".to_owned(), ReadinessStatus::Ok),
            ("scheduler_alive".to_owned(), ReadinessStatus::Ok),
        ]
    );
    root.finish().await;
}

#[tokio::test]
async fn database_readiness_fails_once_the_database_is_gone() {
    // Arrange
    let root = Root::new().await;
    let check = root
        .services
        .get::<DatabaseReady>()
        .await
        .expect("database check");
    let Root { services, db, .. } = root;

    // Act
    db.drop_db().await;
    let item = check.check().await;

    // Assert
    assert_eq!(item.status, ReadinessStatus::Failing);
    drop(services);
}

#[tokio::test]
async fn the_scheduler_is_built_once_for_every_consumer_and_started_once() {
    // Arrange
    let root = Root::new().await;

    // Act
    let first = root.services.get::<Scheduler>().await.expect("scheduler");
    root.services
        .get::<ReadinessChecks>()
        .await
        .expect("readiness");
    let started = root::start_scheduler(&root.services)
        .await
        .expect("scheduler starts");
    let again = root::start_scheduler(&root.services).await;

    // Assert
    assert!(Arc::ptr_eq(&first, &started));
    assert!(again.is_err(), "a second start is refused");
    root.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_registered_janitor_cycles_at_200ms_and_readyz_lists_scheduler_alive() {
    // Arrange: IT-12-7 registration part, in process.
    let fast: Vec<(&str, &str)> = INTERVALS.iter().map(|(key, _)| (*key, "200ms")).collect();
    let root = Root::with_intervals(&fast).await;
    let janitors = root.services.get::<Janitors>().await.expect("janitors");
    let names: Vec<&'static str> = janitors.0.iter().map(|job| job.janitor.name()).collect();
    let checks = root
        .services
        .get::<ReadinessChecks>()
        .await
        .expect("readiness");
    let metrics = root.services.get::<Metrics>().await.expect("metrics");
    root::start_scheduler(&root.services)
        .await
        .expect("scheduler starts");
    let mut registry = ReadinessRegistry::new();
    for check in &checks.0 {
        registry.add(Arc::clone(check));
    }
    let router = ops_router(metrics, registry, Liveness::new());

    // Act
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut scrape = String::new();
    while Instant::now() < deadline {
        scrape = get(&router, "/metrics").await.1;
        if names.iter().all(|name| ok_cycles(&scrape, name) > 0.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (status, body) = get(&router, "/readyz").await;

    // Assert
    let idle: Vec<_> = names
        .iter()
        .filter(|name| ok_cycles(&scrape, name) == 0.0)
        .collect();
    assert!(
        idle.is_empty(),
        "janitors without a successful cycle: {idle:?}\n{scrape}"
    );
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("\"scheduler_alive\""), "{body}");
    assert!(body.contains("\"database\""), "{body}");
    root.finish().await;
}

fn ok_cycles(scrape: &str, task: &str) -> f64 {
    series(
        scrape,
        "sunnycloud_janitor_cycles_total",
        &[("task", task), ("result", "ok")],
    )
    .unwrap_or(0.0)
}
