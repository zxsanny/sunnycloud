//! AZ-3796: `admin epoch-bump` and `admin restore` through the real handler with recording owner ports.

mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use contracts::{
    DoctorCheck, DoctorContext, DoctorTag, EpochAdmin, EpochAdminError, Fault, Finding,
    FindingLevel, HeadReport, MarkerStatus, NodeOrPath, Placement, RestoreError, RestoreOutcome,
    TreeRestore,
};
use sc_clock::{Clock, ManualClock, Timestamp};
use serde_json::Value;
use sunnycloud_server::root::DoctorChecks;
use support::{Captured, Harness, locked};
use uuid::Uuid;

const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

type Events = Arc<Mutex<Vec<String>>>;

struct Epoch {
    events: Events,
    bumps: Mutex<Vec<Timestamp>>,
    answer: Result<i64, ()>,
}

#[async_trait]
impl EpochAdmin for Epoch {
    async fn bump_epoch(&self, now: Timestamp) -> Result<i64, EpochAdminError> {
        locked(&self.bumps).push(now);
        locked(&self.events).push("bump".to_owned());
        self.answer
            .map_err(|()| EpochAdminError::Unexpected(Fault::from("marker write failed")))
    }

    async fn marker_status(&self) -> Result<MarkerStatus, EpochAdminError> {
        unreachable!("epoch-bump does not read the marker")
    }

    async fn head_report(&self) -> Result<HeadReport, EpochAdminError> {
        unreachable!("epoch-bump does not read the heads")
    }
}

struct Check {
    name: &'static str,
    tags: Vec<DoctorTag>,
    findings: Vec<Finding>,
    events: Events,
}

#[async_trait]
impl DoctorCheck for Check {
    fn name(&self) -> &'static str {
        self.name
    }

    fn tags(&self) -> &[DoctorTag] {
        &self.tags
    }

    async fn run(&self, _: &DoctorContext) -> Vec<Finding> {
        locked(&self.events).push(format!("check {}", self.name));
        self.findings.clone()
    }
}

fn finding(level: FindingLevel, message: &str) -> Finding {
    Finding {
        level,
        message: message.to_owned(),
        remedy: "see the restore drill".to_owned(),
    }
}

fn epoch_run(answer: Result<i64, ()>, args: &[&str]) -> (Captured, Arc<Epoch>, Events) {
    let events: Events = Arc::default();
    let epoch = Arc::new(Epoch {
        events: Arc::clone(&events),
        bumps: Mutex::default(),
        answer,
    });
    let checks = DoctorChecks(vec![
        Arc::new(Check {
            name: "missing_blobs",
            tags: vec![DoctorTag::Restore],
            findings: vec![
                finding(FindingLevel::Fail, "revision r1 has no blob"),
                finding(FindingLevel::Fail, "revision r2 has no blob"),
            ],
            events: Arc::clone(&events),
        }),
        Arc::new(Check {
            name: "orphans",
            tags: vec![DoctorTag::Restore],
            findings: vec![finding(FindingLevel::Warn, "one orphan file")],
            events: Arc::clone(&events),
        }),
        Arc::new(Check {
            name: "volume",
            tags: vec![DoctorTag::PreFlight],
            findings: vec![finding(FindingLevel::Ok, "volume is writable")],
            events: Arc::clone(&events),
        }),
    ]);
    let served = Arc::clone(&epoch);
    let result = Harness::new()
        .real_handler()
        .service::<dyn EpochAdmin, _, _>(move |_| {
            let epoch: Arc<dyn EpochAdmin> = served.clone();
            async move { Ok(epoch) }
        })
        .service::<dyn Clock, _, _>(|_| async {
            Ok(Arc::new(ManualClock::new(NOW)) as Arc<dyn Clock>)
        })
        .service::<DoctorChecks, _, _>(move |_| {
            let list = DoctorChecks(checks.0.clone());
            async move { Ok(Arc::new(list)) }
        })
        .run(args);
    (result, epoch, events)
}

#[test]
fn epoch_bump_on_a_non_tty_without_yes_exits_3_and_makes_no_call() {
    let (result, epoch, events) = epoch_run(Ok(7), &["admin", "epoch-bump"]);

    assert_eq!(result.exit, 3, "{}", result.stderr);
    assert!(locked(&epoch.bumps).is_empty());
    assert!(locked(&events).is_empty());
}

#[test]
fn epoch_bump_calls_once_with_the_clock_then_runs_only_the_restore_checks_and_exits_0() {
    let (result, epoch, events) = epoch_run(Ok(7), &["admin", "epoch-bump", "--yes"]);

    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert_eq!(*locked(&epoch.bumps), vec![NOW]);
    assert_eq!(
        *locked(&events),
        ["bump", "check missing_blobs", "check orphans"],
        "the bump first, the restore-tagged checks after it, nothing else"
    );
    assert!(result.stdout.contains("epoch: 7"), "{}", result.stdout);
    for part in [
        "revision r1 has no blob",
        "revision r2 has no blob",
        "one orphan file",
    ] {
        assert!(result.stdout.contains(part), "{part}: {}", result.stdout);
    }
}

#[test]
fn epoch_bump_json_carries_the_epoch_and_the_three_findings() {
    let (result, ..) = epoch_run(Ok(7), &["--json", "admin", "epoch-bump", "--yes"]);

    let envelope: Value = serde_json::from_str(&result.stdout).expect("one JSON object");
    assert_eq!(envelope["data"]["epoch"], 7);
    assert_eq!(
        envelope["data"]["findings"].as_array().map(Vec::len),
        Some(3)
    );
}

#[test]
fn a_bump_error_exits_1_with_the_advice_to_run_again_and_runs_no_check() {
    let (result, epoch, events) = epoch_run(Err(()), &["admin", "epoch-bump", "--yes"]);

    assert_eq!(result.exit, 1);
    assert!(
        result.stderr.contains("run `admin epoch-bump` again"),
        "{}",
        result.stderr
    );
    assert_eq!(locked(&epoch.bumps).len(), 1);
    assert_eq!(*locked(&events), ["bump"]);
}

struct Restorer {
    calls: Mutex<Vec<(Uuid, NodeOrPath, Timestamp)>>,
    previews: Mutex<Vec<(Uuid, NodeOrPath, Timestamp)>>,
    answer: fn() -> Result<RestoreOutcome, RestoreError>,
}

#[async_trait]
impl TreeRestore for Restorer {
    async fn restore_revision(
        &self,
        account: Uuid,
        target: NodeOrPath,
        at: Timestamp,
    ) -> Result<RestoreOutcome, RestoreError> {
        locked(&self.calls).push((account, target, at));
        (self.answer)()
    }

    async fn preview_restore(
        &self,
        account: Uuid,
        target: NodeOrPath,
        at: Timestamp,
    ) -> Result<RestoreOutcome, RestoreError> {
        locked(&self.previews).push((account, target, at));
        (self.answer)()
    }
}

fn restore_run(
    answer: fn() -> Result<RestoreOutcome, RestoreError>,
    args: &[&str],
) -> (Captured, Arc<Restorer>) {
    let restorer = Arc::new(Restorer {
        calls: Mutex::default(),
        previews: Mutex::default(),
        answer,
    });
    let served = Arc::clone(&restorer);
    let result = Harness::new()
        .real_handler()
        .service::<dyn TreeRestore, _, _>(move |_| {
            let restore: Arc<dyn TreeRestore> = served.clone();
            async move { Ok(restore) }
        })
        .run(args);
    (result, restorer)
}

const REVISION: Uuid = Uuid::from_u128(0x1234);

fn restored(placement: Placement) -> Result<RestoreOutcome, RestoreError> {
    Ok(RestoreOutcome {
        restored_revision: REVISION,
        placement,
    })
}

fn restore_args<'a>(account: &'a str, at: &'a str) -> Vec<&'a str> {
    vec![
        "admin",
        "restore",
        "--account",
        account,
        "--path",
        "/x",
        "--at",
        at,
    ]
}

#[test]
fn a_restore_prints_the_revision_and_placement_and_exits_0() {
    let account = Uuid::from_u128(9).to_string();
    let original = restore_run(
        || restored(Placement::Original),
        &restore_args(&account, "2026-11-01T10:00:00Z"),
    );
    let relocated = restore_run(
        || restored(Placement::Restored),
        &restore_args(&account, "2026-11-01T10:00:00Z"),
    );

    assert_eq!(original.0.exit, 0, "{}", original.0.stderr);
    assert!(original.0.stdout.contains(&format!("revision: {REVISION}")));
    assert!(original.0.stdout.contains("placement: original"));
    assert!(relocated.0.stdout.contains("placement: restored"));
    let calls = locked(&original.1.calls);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, Uuid::from_u128(9));
    assert_eq!(calls[0].1, NodeOrPath::Path("/x".to_owned()));
    assert_eq!(
        calls[0].2,
        Timestamp::from_unix_micros(1_793_527_200_000_000)
    );
}

#[test]
fn beyond_retention_and_not_found_exit_2_with_their_codes() {
    let account = Uuid::from_u128(9).to_string();
    let args = restore_args(&account, "2026-11-01T10:00:00Z");

    let beyond = restore_run(|| Err(RestoreError::BeyondRetention), &args);
    let missing = restore_run(|| Err(RestoreError::NotFound), &args);
    let json = restore_run(
        || Err(RestoreError::BeyondRetention),
        &[&["--json"][..], &args[..]].concat(),
    );

    assert_eq!(beyond.0.exit, 2);
    assert_eq!(missing.0.exit, 2);
    let envelope: Value = serde_json::from_str(&json.0.stdout).expect("envelope");
    assert_eq!(envelope["code"], "beyond_retention");
    assert_eq!(envelope["result"], "failed");
}

#[test]
fn a_bad_time_account_or_missing_path_exits_3_with_no_call() {
    let account = Uuid::from_u128(9).to_string();
    let cases: Vec<Vec<&str>> = vec![
        restore_args(&account, "2026-11-01 10:00"),
        restore_args(&account, "2026-11-01T10:00:00+02:00"),
        restore_args("not-an-id", "2026-11-01T10:00:00Z"),
        vec![
            "admin",
            "restore",
            "--account",
            &account,
            "--at",
            "2026-11-01T10:00:00Z",
        ],
    ];

    for args in cases {
        let (result, restorer) = restore_run(|| restored(Placement::Original), &args);

        assert_eq!(result.exit, 3, "{args:?}: {}", result.stderr);
        assert!(locked(&restorer.calls).is_empty(), "{args:?}");
    }
}

#[test]
fn dry_run_previews_with_the_same_arguments_and_says_nothing_was_changed() {
    // Arrange
    let account = Uuid::from_u128(9).to_string();
    let mut args = restore_args(&account, "2026-11-01T10:00:00Z");
    args.push("--dry-run");
    let mut json_args = vec!["--json"];
    json_args.extend(&args);

    // Act
    let (result, restorer) = restore_run(|| restored(Placement::Restored), &args);
    let (object, _) = restore_run(|| restored(Placement::Restored), &json_args);

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert!(
        result.stdout.contains("changed: false"),
        "{}",
        result.stdout
    );
    assert!(result.stdout.contains("placement: restored"));
    assert!(result.stdout.contains(&format!("revision: {REVISION}")));
    assert!(locked(&restorer.calls).is_empty(), "no real restore");
    let previews = locked(&restorer.previews);
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].0, Uuid::from_u128(9));
    assert_eq!(previews[0].1, NodeOrPath::Path("/x".to_owned()));
    let envelope: Value = serde_json::from_str(&object.stdout).expect("envelope");
    assert_eq!(envelope["data"]["changed"], false);
}

#[test]
fn a_real_restore_says_it_changed_something_and_a_dry_run_refusal_is_the_real_one() {
    // Arrange
    let account = Uuid::from_u128(9).to_string();
    let plain = restore_args(&account, "2026-11-01T10:00:00Z");
    let mut dry = plain.clone();
    dry.push("--dry-run");

    // Act
    let (real, _) = restore_run(|| restored(Placement::Original), &plain);
    let (refused, restorer) = restore_run(|| Err(RestoreError::BeyondRetention), &dry);

    // Assert
    assert!(real.stdout.contains("changed: true"), "{}", real.stdout);
    assert_eq!(refused.exit, 2);
    assert!(locked(&restorer.calls).is_empty());
    assert_eq!(locked(&restorer.previews).len(), 1);
}
