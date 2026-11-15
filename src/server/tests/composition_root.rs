//! AZ-3789: the composition root built for `serve` against PostgreSQL 18 and a blob volume. AC-2 (late lists in
//! their documented order), AC-3 (one limiter, one scheduler), AC-4 (only what a command needs, each service
//! once), AC-6 (no vault item in a default build; the crate-graph half is in `crate_graph`). AZ-4311 AC-3 (03's
//! commit effects hold the one shared media effect).

mod janitor_support;
mod root_support;

use std::any::type_name;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use contracts::{MediaAdmin, Scrubber};
use janitor_support::{get, series};
use media_api::MediaAdminService;
use sc_observability::{Liveness, Metrics, ReadinessCheck as _, ReadinessRegistry, ops_router};

use sc_config::parse_duration;
use sc_observability::ReadinessStatus;
use sc_ratelimit::AbuseLimiter;
use sunnycloud_blobstore::MasterKeyring;
use sunnycloud_server::janitor::Scheduler;
use sunnycloud_server::root::{
    self, AccountHooks, Admin, BlobCleanups, Blobstore, CommitEffects, DatabaseReady, DoctorChecks,
    Feed, HealthContributors, Identity, Janitors, Media, MediaPlanning, Quota, ReadinessChecks,
    Sharing, Transfer, Tree,
};

use root_support::{INTERVALS, Root, address};

fn interval(key: &str) -> Duration {
    let (_, text) = INTERVALS
        .iter()
        .find(|(name, _)| *name == key)
        .unwrap_or_else(|| panic!("{key} has no test interval"));
    parse_duration(text).expect("a duration")
}

/// 06, (20 in a `vault` build), 02, 03, 08, 05, 07a: each entry is the owner's own hook instance.
#[tokio::test]
async fn hook_list_follows_the_documented_order_and_reaches_the_owners_hooks() {
    // Arrange
    let root = Root::new().await;

    // Act
    let hooks = root
        .services
        .get::<AccountHooks>()
        .await
        .expect("hook list");

    // Assert
    let services = &root.services;
    let sharing = services.get::<Sharing>().await.expect("06");
    let tree = services.get::<Tree>().await.expect("02");
    let transfer = services.get::<Transfer>().await.expect("03");
    let media = services.get::<Media>().await.expect("08");
    let feed = services.get::<Feed>().await.expect("05");
    let quota = services.get::<Quota>().await.expect("07a");
    let mut expected = vec![("06", address(&sharing.lifecycle))];
    #[cfg(feature = "vault")]
    {
        let vault = services
            .get::<sunnycloud_server::root::Vault>()
            .await
            .expect("20");
        expected.push(("20", address(&vault.lifecycle)));
    }
    expected.extend([
        ("02", address(&tree.lifecycle)),
        ("03", address(&transfer.lifecycle)),
        ("08", address(&media.lifecycle)),
        ("05", address(&feed.lifecycle)),
        ("07a", address(&quota.admin)),
    ]);
    let actual: Vec<_> = hooks
        .0
        .iter()
        .map(|hook| (hook.owner, address(&hook.item)))
        .collect();
    assert_eq!(actual, expected);
    root.finish().await;
}

#[tokio::test]
async fn cleanup_list_is_02_then_08() {
    // Arrange
    let root = Root::new().await;

    // Act
    let cleanups = root.services.get::<BlobCleanups>().await.expect("cleanups");

    // Assert
    let tree = root.services.get::<Tree>().await.expect("02");
    let media = root.services.get::<Media>().await.expect("08");
    let actual: Vec<_> = cleanups
        .0
        .iter()
        .map(|entry| (entry.owner, address(&entry.item)))
        .collect();
    assert_eq!(
        actual,
        [
            ("02", address(&tree.lifecycle)),
            ("08", address(&media.lifecycle))
        ]
    );
    root.finish().await;
}

/// AZ-4311 AC-3: one 08 media effect for every account, and 03 is built with that instance.
#[tokio::test]
async fn transfer_commit_effects_hold_the_one_shared_media_effect() {
    // Arrange
    let root = Root::new().await;
    let effects = root
        .services
        .get::<CommitEffects>()
        .await
        .expect("commit effects");
    let media = Arc::clone(&effects.0[0].item);
    let before = Arc::strong_count(&media);

    // Act
    root.services.get::<Transfer>().await.expect("03");

    // Assert
    let owners: Vec<_> = effects.0.iter().map(|entry| entry.owner).collect();
    assert_eq!(owners, ["08"]);
    assert!(
        Arc::strong_count(&media) > before,
        "03 holds the list's media effect"
    );
    let again = root
        .services
        .get::<CommitEffects>()
        .await
        .expect("commit effects");
    assert!(Arc::ptr_eq(&effects, &again));
    root.finish().await;
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
async fn health_and_doctor_lists_hold_the_owners_items() {
    // Arrange
    let root = Root::new().await;

    // Act
    let health = root
        .services
        .get::<HealthContributors>()
        .await
        .expect("health");
    let doctor = root.services.get::<DoctorChecks>().await.expect("doctor");

    // Assert
    let health: Vec<_> = health.0.iter().map(|item| item.name().to_owned()).collect();
    let doctor: Vec<_> = doctor.0.iter().map(|check| check.name()).collect();
    assert_eq!(health, ["blobstore", "media"]);
    assert_eq!(
        doctor,
        [
            "master_key",
            "blob_volume",
            "blob_retention",
            "blob_files_missing",
            "media_worker"
        ]
    );
    root.finish().await;
}

#[tokio::test]
async fn identity_and_sharing_hold_the_one_limiter_of_the_context() {
    // Arrange
    let root = Root::new().await;
    let limiter = root.services.get::<AbuseLimiter>().await.expect("limiter");
    let before = Arc::strong_count(&limiter);

    // Act
    root.services.get::<Identity>().await.expect("01");
    let after_identity = Arc::strong_count(&limiter);
    root.services.get::<Sharing>().await.expect("06");
    let after_sharing = Arc::strong_count(&limiter);

    // Assert
    assert!(after_identity > before, "01 holds the context's limiter");
    assert!(
        after_sharing > after_identity,
        "06 holds the context's limiter"
    );
    let again = root.services.get::<AbuseLimiter>().await.expect("limiter");
    assert!(Arc::ptr_eq(&limiter, &again));
    root.finish().await;
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

#[tokio::test]
async fn key_check_builds_the_keyring_and_nothing_of_another_component() {
    // Arrange
    let root = Root::new().await;

    // Act
    root.services.get::<MasterKeyring>().await.expect("keyring");

    // Assert
    let mut expected = vec![
        type_name::<sc_config::ConfigSources>(),
        type_name::<sunnycloud_server::ServeSettings>(),
        type_name::<sc_db::Db>(),
        type_name::<sc_audit::AuditWriter>(),
        type_name::<dyn sc_clock::Clock>(),
        type_name::<MasterKeyring>(),
    ];
    expected.sort_unstable();
    assert_eq!(root.services.built(), expected);
    root.finish().await;
}

#[tokio::test]
async fn media_admin_builds_without_the_tree_or_the_blobs_and_media_holds_the_same_instance() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    let admin = services.get::<dyn MediaAdmin>().await.expect("media admin");
    let stats = admin.stats().await.expect("stats on an empty queue");

    // Assert
    let built = services.built();
    assert!(stats.counts.iter().all(|item| item.count == 0));
    assert!(!built.contains(&type_name::<Tree>()));
    assert!(!built.contains(&type_name::<Blobstore>()));
    let media = services.get::<Media>().await.expect("08");
    assert!(Arc::ptr_eq(
        &media.admin,
        &services
            .get::<MediaAdminService>()
            .await
            .expect("admin service")
    ));
    root.finish().await;
}

#[tokio::test]
async fn the_scrubber_service_is_the_instance_04_holds() {
    // Arrange
    let root = Root::new().await;

    // Act
    let scrubber = root.services.get::<dyn Scrubber>().await.expect("scrubber");
    let blobs = root.services.get::<Blobstore>().await.expect("04");

    // Assert
    assert!(std::ptr::addr_eq(
        Arc::as_ptr(&scrubber),
        Arc::as_ptr(&blobs.scrubber)
    ));
    root.finish().await;
}

#[tokio::test]
async fn serve_builds_every_component_once_although_several_consumers_receive_it() {
    // Arrange
    let root = Root::new().await;
    let services = &root.services;

    // Act
    services.get::<ReadinessChecks>().await.expect("readiness");
    services.get::<DoctorChecks>().await.expect("doctor");
    services.get::<Admin>().await.expect("07");
    let tree = services.get::<Tree>().await.expect("02");
    let blobs = services.get::<Blobstore>().await.expect("04");

    // Assert: every later request answers the instance the consumers were built with.
    let planning = services.get::<MediaPlanning>().await.expect("08 planning");
    let media = services.get::<Media>().await.expect("08");
    let hooks = services.get::<AccountHooks>().await.expect("hooks");
    let cleanups = services.get::<BlobCleanups>().await.expect("cleanups");
    assert!(Arc::ptr_eq(
        &tree,
        &services.get::<Tree>().await.expect("02")
    ));
    assert!(Arc::ptr_eq(
        &blobs,
        &services.get::<Blobstore>().await.expect("04")
    ));
    let hook_of = |owner| {
        let hook = hooks
            .0
            .iter()
            .find(|hook| hook.owner == owner)
            .expect("hook");
        address(&hook.item)
    };
    assert_eq!(hook_of("02"), address(&cleanups.0[0].item));
    assert_eq!(hook_of("08"), address(&cleanups.0[1].item));
    assert_eq!(hook_of("08"), address(&media.lifecycle));
    assert!(Arc::ptr_eq(
        &planning,
        &services.get::<MediaPlanning>().await.expect("08")
    ));
    let built = services.built();
    let mut unique = built.clone();
    unique.dedup();
    assert_eq!(built, unique);
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
