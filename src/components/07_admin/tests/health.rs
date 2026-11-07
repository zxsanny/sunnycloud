//! AC-1 to AC-5 of the health aggregation. The contributors are test contributors (IT-07-6). The checks that
//! sleep run on tokio's paused clock, so elapsed times are virtual and exact.

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use contracts::{Actor, HealthItem, HealthReport, HealthStatus, PrincipalKind, Role};
use sunnycloud_admin::AdminCallError;
use support::harness::{Harness, admin_actor, principal};
use support::health_doubles::{TestContributor, as_ports, six};
use tokio::time::Instant;
use uuid::Uuid;

const TIMEOUT_300_MS: [(&str, &str); 1] = [("SUNNYCLOUD_HEALTH_CHECK_TIMEOUT", "300ms")];

fn operator_report(report: Result<HealthReport, AdminCallError>) -> Vec<(String, HealthItem)> {
    report.expect("the operator may read the health").items
}

fn named<'a>(items: &'a [(String, HealthItem)], name: &str) -> &'a HealthItem {
    &items
        .iter()
        .find(|(item_name, _)| item_name == name)
        .unwrap_or_else(|| panic!("{name} is missing from {items:?}"))
        .1
}

fn is_ok(items: &[(String, HealthItem)]) -> bool {
    items
        .iter()
        .all(|(_, item)| item.status == HealthStatus::Ok)
}

#[tokio::test]
async fn every_contributor_is_reported_by_name_and_the_report_is_not_ok() {
    // Arrange
    let contributors = six();
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;

    // Act
    let items = operator_report(harness.service.health(&Actor::Operator).await);

    // Assert
    let expected = [
        ("database", HealthStatus::Ok, "reachable"),
        (
            "blob volume",
            HealthStatus::Degraded,
            "below the free floor",
        ),
        ("master key", HealthStatus::Ok, "loaded"),
        ("scrub", HealthStatus::Ok, "last run clean"),
        ("dead jobs", HealthStatus::Down, "3 dead jobs"),
        ("janitor lag", HealthStatus::Ok, "on time"),
    ];
    let expected: Vec<(String, HealthItem)> = expected
        .into_iter()
        .map(|(name, status, detail)| (name.to_owned(), HealthItem::new(status, detail)))
        .collect();
    assert_eq!(items, expected);
    assert!(!is_ok(&items));
    harness.finish().await;
}

#[tokio::test]
async fn a_report_of_healthy_contributors_is_ok() {
    // Arrange
    let contributors = vec![
        TestContributor::answering("database", HealthStatus::Ok, ""),
        TestContributor::answering("scrub", HealthStatus::Ok, ""),
    ];
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;

    // Act
    let items = operator_report(harness.service.health(&Actor::Operator).await);

    // Assert
    assert_eq!(items.len(), 2);
    assert!(is_ok(&items));
    harness.finish().await;
}

#[tokio::test]
async fn a_sleeping_contributor_is_down_and_timed_out_and_the_others_are_kept() {
    // Arrange
    let mut contributors = six();
    contributors.push(TestContributor::sleeping("sleeper", Duration::from_secs(5)));
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;
    tokio::time::pause();

    // Act
    let started = Instant::now();
    let items = operator_report(harness.service.health(&Actor::Operator).await);
    let elapsed = started.elapsed();

    // Assert
    tokio::time::resume();
    let sleeper = named(&items, "sleeper");
    assert_eq!(sleeper.status, HealthStatus::Down);
    assert!(sleeper.detail.contains("timed out"), "{sleeper:?}");
    assert_eq!(items.len(), 7);
    assert_eq!(named(&items, "blob volume").status, HealthStatus::Degraded);
    assert_eq!(named(&items, "database").status, HealthStatus::Ok);
    assert_eq!(named(&items, "dead jobs").detail, "3 dead jobs");
    assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
    harness.finish().await;
}

#[tokio::test]
async fn several_sleeping_contributors_are_bounded_together_not_one_after_another() {
    // Arrange
    let contributors = vec![
        TestContributor::sleeping("first", Duration::from_secs(5)),
        TestContributor::sleeping("second", Duration::from_secs(5)),
        TestContributor::sleeping("third", Duration::from_secs(5)),
    ];
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;
    tokio::time::pause();

    // Act
    let started = Instant::now();
    let items = operator_report(harness.service.health(&Actor::Operator).await);
    let elapsed = started.elapsed();

    // Assert
    tokio::time::resume();
    assert!(
        items
            .iter()
            .all(|(_, item)| item.status == HealthStatus::Down)
    );
    assert_eq!(items.len(), 3);
    assert!(elapsed < Duration::from_millis(600), "{elapsed:?}");
    harness.finish().await;
}

#[tokio::test]
async fn a_contributor_that_answers_within_the_timeout_is_not_cut_short() {
    // Arrange
    let contributors = vec![TestContributor::sleeping(
        "slow but in time",
        Duration::from_millis(299),
    )];
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;
    tokio::time::pause();

    // Act
    let items = operator_report(harness.service.health(&Actor::Operator).await);

    // Assert
    tokio::time::resume();
    assert_eq!(named(&items, "slow but in time").status, HealthStatus::Ok);
    harness.finish().await;
}

#[tokio::test]
async fn an_unset_timeout_still_bounds_a_sleeping_contributor() {
    // Arrange
    let contributors = vec![
        TestContributor::answering("database", HealthStatus::Ok, ""),
        TestContributor::sleeping("sleeper", Duration::from_secs(3600)),
    ];
    let harness = Harness::with_health(&[], as_ports(&contributors)).await;
    tokio::time::pause();

    // Act
    let started = Instant::now();
    let items = operator_report(harness.service.health(&Actor::Operator).await);
    let elapsed = started.elapsed();

    // Assert
    tokio::time::resume();
    assert_eq!(named(&items, "sleeper").status, HealthStatus::Down);
    assert_eq!(named(&items, "database").status, HealthStatus::Ok);
    assert!(elapsed < Duration::from_secs(3600), "{elapsed:?}");
    harness.finish().await;
}

#[tokio::test]
async fn a_failing_contributor_is_down_under_its_name_and_the_others_are_kept() {
    // Arrange
    let mut contributors = six();
    contributors.insert(2, TestContributor::panicking("flaky"));
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;

    // Act
    let items = operator_report(harness.service.health(&Actor::Operator).await);

    // Assert
    let names: Vec<&str> = items.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "database",
            "blob volume",
            "flaky",
            "master key",
            "scrub",
            "dead jobs",
            "janitor lag"
        ]
    );
    let flaky = named(&items, "flaky");
    assert_eq!(flaky.status, HealthStatus::Down);
    assert_eq!(flaky.detail, "check failed");
    assert_eq!(named(&items, "master key").status, HealthStatus::Ok);
    assert_eq!(named(&items, "scrub").detail, "last run clean");
    harness.finish().await;
}

#[tokio::test]
async fn a_degraded_contributor_shows_degraded() {
    // Arrange
    let contributors = vec![
        TestContributor::answering("database", HealthStatus::Ok, ""),
        TestContributor::answering(
            "blob volume",
            HealthStatus::Degraded,
            "2 GiB above the floor",
        ),
    ];
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;

    // Act
    let items = operator_report(harness.service.health(&Actor::Operator).await);

    // Assert
    assert_eq!(
        named(&items, "blob volume"),
        &HealthItem::new(HealthStatus::Degraded, "2 GiB above the floor")
    );
    assert!(!is_ok(&items));
    harness.finish().await;
}

#[tokio::test]
async fn a_user_session_and_an_admin_device_token_are_forbidden_and_no_check_runs() {
    // Arrange
    let contributors = six();
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;
    let account = Uuid::new_v4();
    let callers = [
        Actor::Admin(principal(account, PrincipalKind::Session, Role::User)),
        Actor::Admin(principal(account, PrincipalKind::Device, Role::Admin)),
    ];

    for caller in callers {
        // Act
        let result = harness.service.health(&caller).await;

        // Assert
        assert!(
            matches!(result, Err(AdminCallError::Forbidden)),
            "{result:?}"
        );
    }
    for contributor in &contributors {
        assert_eq!(contributor.calls.load(Ordering::SeqCst), 0);
    }
    harness.finish().await;
}

#[tokio::test]
async fn an_admin_session_and_the_operator_are_accepted_and_each_call_runs_every_check() {
    // Arrange
    let contributors = six();
    let harness = Harness::with_health(&TIMEOUT_300_MS, as_ports(&contributors)).await;
    let admin = admin_actor(Uuid::new_v4());

    for caller in [&admin, &Actor::Operator] {
        // Act
        let items = operator_report(harness.service.health(caller).await);

        // Assert
        assert_eq!(items.len(), 6);
    }
    for contributor in &contributors {
        assert_eq!(contributor.calls.load(Ordering::SeqCst), 2);
    }
    harness.finish().await;
}
