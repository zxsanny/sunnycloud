//! AZ-3807: `doctor` aggregation through the real handler with recording checks.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};
use serde_json::Value;
use sunnycloud_server::root::{DoctorChecks, PreFlightChecks};
use sunnycloud_server::{CheckSelection, run_checks};
use support::{Captured, Harness, locked};

struct Recording {
    name: &'static str,
    tags: Vec<DoctorTag>,
    findings: Vec<Finding>,
    contexts: Arc<Mutex<Vec<DoctorContext>>>,
}

#[async_trait]
impl DoctorCheck for Recording {
    fn name(&self) -> &'static str {
        self.name
    }

    fn tags(&self) -> &[DoctorTag] {
        &self.tags
    }

    async fn run(&self, ctx: &DoctorContext) -> Vec<Finding> {
        locked(&self.contexts).push(*ctx);
        self.findings.clone()
    }
}

fn finding(level: FindingLevel, message: &str, remedy: &str) -> Finding {
    Finding {
        level,
        message: message.to_owned(),
        remedy: remedy.to_owned(),
    }
}

fn check(
    name: &'static str,
    tags: &[DoctorTag],
    findings: Vec<Finding>,
    contexts: &Arc<Mutex<Vec<DoctorContext>>>,
) -> Arc<dyn DoctorCheck> {
    Arc::new(Recording {
        name,
        tags: tags.to_vec(),
        findings,
        contexts: Arc::clone(contexts),
    })
}

fn run(checks: Vec<Arc<dyn DoctorCheck>>, args: &[&str]) -> Captured {
    Harness::new()
        .real_handler()
        .service::<DoctorChecks, _, _>(move |_| {
            let checks = DoctorChecks(checks.clone());
            async move { Ok(Arc::new(checks)) }
        })
        .run(args)
}

/// Only the pre-flight list is registered: a run that asked for any other list would fail.
fn run_pre_flight(checks: Vec<Arc<dyn DoctorCheck>>, args: &[&str]) -> Captured {
    Harness::new()
        .real_handler()
        .service::<PreFlightChecks, _, _>(move |_| {
            let checks = PreFlightChecks(checks.clone());
            async move { Ok(Arc::new(checks)) }
        })
        .run(args)
}

fn contexts() -> Arc<Mutex<Vec<DoctorContext>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn ok_check(seen: &Arc<Mutex<Vec<DoctorContext>>>) -> Arc<dyn DoctorCheck> {
    check(
        "volume",
        &[],
        vec![finding(FindingLevel::Ok, "volume is writable", "")],
        seen,
    )
}

fn warn_check(seen: &Arc<Mutex<Vec<DoctorContext>>>) -> Arc<dyn DoctorCheck> {
    check(
        "backup_age",
        &[],
        vec![finding(
            FindingLevel::Warn,
            "last backup is old",
            "run a backup",
        )],
        seen,
    )
}

fn fail_check(seen: &Arc<Mutex<Vec<DoctorContext>>>) -> Arc<dyn DoctorCheck> {
    check(
        "master_key",
        &[DoctorTag::PreFlight],
        vec![finding(
            FindingLevel::Fail,
            "master key does not match",
            "mount the right key",
        )],
        seen,
    )
}

fn json_findings(result: &Captured) -> Vec<Value> {
    let envelope: Value = serde_json::from_str(&result.stdout).expect("one JSON object");
    envelope["data"]["findings"]
        .as_array()
        .expect("findings array")
        .clone()
}

#[test]
fn levels_map_to_exit_0_then_1_then_2_and_each_finding_prints_all_four_parts() {
    let seen = contexts();
    let only_ok = run(vec![ok_check(&seen)], &["doctor"]);
    let one_warn = run(vec![ok_check(&seen), warn_check(&seen)], &["doctor"]);
    let warn_and_fail = run(vec![warn_check(&seen), fail_check(&seen)], &["doctor"]);

    assert_eq!(only_ok.exit, 0);
    assert_eq!(one_warn.exit, 1);
    assert_eq!(warn_and_fail.exit, 2);
    for part in [
        "check: backup_age",
        "level: warn",
        "message: last backup is old",
        "remedy: run a backup",
        "check: master_key",
        "level: fail",
        "remedy: mount the right key",
    ] {
        assert!(
            warn_and_fail.stdout.contains(part),
            "{part}: {}",
            warn_and_fail.stdout
        );
    }
}

#[test]
fn json_carries_the_same_findings_levels_remedies_and_exit_as_the_text() {
    let seen = contexts();
    let checks = vec![warn_check(&seen), fail_check(&seen)];

    let text = run(checks.clone(), &["doctor"]);
    let json = run(checks, &["--json", "doctor"]);

    assert_eq!(json.exit, text.exit);
    let items = json_findings(&json);
    assert_eq!(items.len(), 2);
    for item in &items {
        for field in ["check", "level", "message", "remedy"] {
            let value = item[field].as_str().expect("string field");
            assert!(
                text.stdout.contains(&format!("{field}: {value}")),
                "{field}={value} missing in {}",
                text.stdout
            );
        }
    }
    assert_eq!(items[0]["level"], "warn");
    assert_eq!(items[1]["level"], "fail");
}

#[test]
fn pre_flight_runs_the_pre_flight_list_and_builds_no_other() {
    // Arrange
    let seen = contexts();
    let checks = vec![check(
        "redirect_uris",
        &[DoctorTag::PreFlight],
        vec![finding(FindingLevel::Ok, "https://cloud.example/cb", "")],
        &seen,
    )];

    // Act
    let result = run_pre_flight(checks, &["doctor", "--pre-flight"]);

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert!(result.stdout.contains("https://cloud.example/cb"));
    assert_eq!(locked(&seen).len(), 1);
    assert!(locked(&seen)[0].pre_flight);
}

#[test]
fn pre_flight_skips_a_check_in_its_list_that_is_not_tagged_pre_flight() {
    // Arrange
    let seen = contexts();
    let checks = vec![warn_check(&seen)];

    // Act
    let result = run_pre_flight(checks, &["doctor", "--pre-flight"]);

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert!(locked(&seen).is_empty());
}

#[tokio::test]
async fn a_tagged_subset_runs_only_that_tag_and_returns_its_findings() {
    let seen = contexts();
    let other = contexts();
    let checks = vec![
        check(
            "missing_blobs",
            &[DoctorTag::Restore],
            vec![finding(
                FindingLevel::Fail,
                "3 blobs missing",
                "restore them",
            )],
            &seen,
        ),
        warn_check(&other),
    ];

    let results = run_checks(
        &checks,
        CheckSelection::Tagged(DoctorTag::Restore),
        &DoctorContext::default(),
    )
    .await;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].check, "missing_blobs");
    assert_eq!(results[0].finding.level, FindingLevel::Fail);
    assert!(locked(&other).is_empty());
}

#[test]
fn options_reach_every_check_and_a_bad_duration_exits_3_with_no_check_run() {
    let seen = contexts();
    let checks = vec![ok_check(&seen), warn_check(&seen)];

    let good = run(
        checks.clone(),
        &["doctor", "--backup-interval", "7d", "--attest-backup"],
    );
    let contexts_seen = locked(&seen).clone();
    let bad = run(checks, &["doctor", "--backup-interval", "soon"]);

    assert_eq!(good.exit, 1);
    assert_eq!(contexts_seen.len(), 2);
    for ctx in contexts_seen {
        assert_eq!(ctx.backup_interval, Some(Duration::from_secs(7 * 86_400)));
        assert!(ctx.attest_backup);
        assert!(!ctx.pre_flight);
    }
    assert_eq!(bad.exit, 3);
    assert_eq!(locked(&seen).len(), 2, "no check ran for the bad value");
}

#[test]
fn no_registered_checks_is_a_clean_run() {
    let result = run(Vec::new(), &["doctor"]);

    assert_eq!(result.exit, 0);
}
