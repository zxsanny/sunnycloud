//! AZ-3666 (E-22/p13), flow F14 through the real `sunnycloud` binary on PostgreSQL 18 and a blob volume: a real
//! `pg_dump` and `pg_restore` of the install's database (the dump tools: `pipeline_support::restore`, `SC_PG_TOOLS`),
//! `serve` refusing the restored database until `admin epoch-bump` (AC-1), the bump and the devices' resync
//! (AC-2), the paged listing after it (AC-3), the missing blobs of the restore (AC-73), a wrong master key (AC-72),
//! `admin keycheck-init` for a volume without its key check (F14 step 6) and the production callers (AC-4).
//!
//! The restore scenario (`restored`): a dump at T0; after it three files are deleted and their blobs collected at
//! `retention` 0 by `admin gc-run`, two files are added and `admin epoch-bump` runs once, so the volume's `.epoch`
//! marker is newer than the dump; then the database is wiped and the dump restored while `serve` is stopped. The
//! device's pre-restore cursor is its head before that stop.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::collections::BTreeSet;
use std::fs;

use pipeline_support::feed::{Page, split};
use pipeline_support::restore::{Restored, restored};
use pipeline_support::wiring::production_source;
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, uuid_of};
use serve_support::{messages, positions};
use uuid::Uuid;

/// The start steps whose order is the restore safety: every check before the listeners.
const CHECKS_THEN_LISTENERS: [&str; 4] = [
    "serve start: schema version",
    "serve start: key check",
    "serve start: epoch marker",
    "serve start: listeners",
];

/// `admin epoch-bump --yes`; the new epoch and the run.
fn bump(pipeline: &Pipeline) -> (u64, pipeline_support::restore::Run) {
    let bumped = pipeline.command(&["admin", "epoch-bump", "--yes"]);
    assert_eq!(bumped.code, Some(0), "{bumped:?}");
    let epoch = bumped.envelope()["data"]["epoch"]
        .as_u64()
        .unwrap_or_else(|| panic!("no epoch: {}", bumped.stdout));
    (epoch, bumped)
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_serve_refuses_a_restored_database_below_the_marker_before_any_listener_naming_the_bump()
 {
    // Arrange
    let restored = restored().await;

    // Act
    let refused = restored.pipeline.stack.refused();

    // Assert
    assert!(restored.newer_marker > restored.dump_epoch);
    assert_eq!(
        refused.exited.code,
        Some(2),
        "{}",
        refused.exited.stderr_text()
    );
    assert!(!refused.accepted, "a listener accepted a connection");
    let stderr = refused.exited.stderr_text();
    assert!(stderr.contains("admin epoch-bump"), "{stderr}");
    assert!(
        !messages(&refused.exited.stderr).contains(&"serve start: listeners".to_owned()),
        "{stderr}"
    );
    restored.pipeline.stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_doctor_reports_fail_for_the_epoch_check_of_a_restored_database() {
    // Arrange
    let restored = restored().await;

    // Act
    let doctor = restored.pipeline.doctor();

    // Assert
    let findings = doctor.findings();
    let epoch_fails: Vec<_> = findings
        .iter()
        .filter(|(_, level, message)| level == "fail" && message.contains(".epoch marker"))
        .collect();
    let registered = production_source("server/src/root/lists.rs").contains("epoch_findings");
    assert!(
        !epoch_fails.is_empty(),
        "no epoch check Fail in doctor (05's `epoch_findings` registered by 12: {registered}); findings: \
         {findings:#?}"
    );
    assert_eq!(doctor.code, Some(2), "{}", doctor.stderr);
    restored.pipeline.stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_the_bump_writes_the_marker_serve_gets_ready_and_old_or_ahead_cursors_must_resync() {
    // Arrange
    let Restored {
        mut pipeline,
        old_cursor,
        dump_epoch,
        newer_marker,
        ..
    } = restored().await;

    // Act
    let (epoch, _) = bump(&pipeline);
    pipeline.restart();
    let head = pipeline.feed_cursor();
    let (head_epoch, head_seq) = split(&head);
    let old = pipeline.changes(&pipeline.device, &old_cursor, None, None);
    let ahead = pipeline.changes(
        &pipeline.device,
        &format!("{head_epoch}.{}", head_seq + 1),
        None,
        None,
    );

    // Assert
    assert_eq!(pipeline.marker(), format!("{epoch}\n"));
    assert!(epoch > dump_epoch && epoch >= newer_marker, "{epoch}");
    assert_eq!(head_epoch, epoch);
    let started = positions(&messages(&pipeline.server.lines()), &CHECKS_THEN_LISTENERS);
    assert!(
        started.windows(2).all(|pair| pair[0] < pair[1]),
        "{started:?}"
    );
    for (answer, reason) in [(&old, "epoch_changed"), (&ahead, "cursor_ahead")] {
        assert_eq!(answer.status, 410, "{answer:?}");
        let problem = answer.json();
        assert_eq!(problem["code"], "resync_required", "{problem}");
        assert_eq!(problem["reason"], reason, "{problem}");
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_after_the_resync_a_paged_listing_has_every_live_node_once_and_polling_resumes_at_the_head()
 {
    // Arrange
    let Restored {
        mut pipeline,
        nodes,
        ..
    } = restored().await;
    bump(&pipeline);
    pipeline.restart();
    let docs = nodes[0];

    // Act
    let head = pipeline.feed_cursor();
    let listed = pipeline.walk(2);
    let resumed = Page::of(&pipeline.changes(&pipeline.device, &head, None, None));
    let (new_node, _) = pipeline.publish_small(docs, &File::new("after.txt", 500, 99), None);
    let next = Page::of(&pipeline.changes(&pipeline.device, &head, None, None));

    // Assert
    let ids: Vec<Uuid> = listed.iter().map(|item| uuid_of(item, "node_id")).collect();
    let unique: BTreeSet<Uuid> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "a node is listed twice: {ids:?}");
    assert_eq!(unique, nodes.iter().copied().collect::<BTreeSet<_>>());
    assert!(
        listed.iter().all(|item| item["state"] == "live"),
        "{listed:?}"
    );
    assert!(resumed.items.is_empty(), "{:?}", resumed.items);
    assert_eq!(resumed.next_cursor, head);
    assert_eq!(next.items.len(), 1, "{:?}", next.items);
    assert_eq!(uuid_of(&next.items[0], "node_id"), new_node);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac73_missing_blobs_are_listed_by_doctor_with_ids_and_served_as_404_content_unavailable() {
    // Arrange
    let Restored {
        mut pipeline,
        missing,
        kept,
        ..
    } = restored().await;
    let (_, bumped) = bump(&pipeline);
    pipeline.restart();
    let docs = pipeline.walk(1000);

    // Act
    let doctor = pipeline.doctor();
    let pages: Vec<_> = docs
        .iter()
        .filter(|item| item["kind"] == "folder")
        .map(|folder| uuid_of(folder, "node_id"))
        .flat_map(|folder| pipeline.pages(folder, 1000))
        .collect();
    let reads: Vec<_> = missing
        .iter()
        .flat_map(|file| {
            let path = format!("/api/v1/nodes/{}/content", file.node);
            [
                pipeline.call("GET", &path, &[], &[]),
                pipeline.call("GET", &path, &[], &[]),
            ]
        })
        .collect();
    let healthy = pipeline.download(kept[0].node);

    // Assert
    assert_eq!(doctor.code, Some(2), "{}", doctor.stderr);
    for run in [&doctor, &bumped] {
        let reported: Vec<_> = run
            .findings()
            .into_iter()
            .filter(|(check, _, _)| check == "blob_files_missing")
            .collect();
        assert_eq!(reported.len(), missing.len(), "{reported:#?}");
        for file in &missing {
            let line = reported
                .iter()
                .find(|(_, _, message)| message.contains(&file.revision.to_string()))
                .unwrap_or_else(|| panic!("{} not listed: {reported:#?}", file.revision));
            assert_eq!(line.1, "fail");
            assert!(
                line.2.contains(&pipeline.account.id.to_string()),
                "{}",
                line.2
            );
            assert!(line.2.contains(&file.path), "{}", line.2);
        }
    }
    let listed: BTreeSet<Uuid> = pages
        .iter()
        .flatten()
        .map(|item| uuid_of(item, "node_id"))
        .collect();
    assert!(
        missing.iter().all(|file| listed.contains(&file.node)),
        "{listed:?}"
    );
    for read in &reads {
        assert_eq!(read.status, 404, "{read:?}");
        assert_eq!(read.code(), "content_unavailable", "{read:?}");
    }
    for file in &missing {
        assert!(
            pipeline.blob_flagged(file.blob).await,
            "{} not flagged",
            file.blob
        );
    }
    assert_eq!(healthy, kept[0].bytes);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac72_a_wrong_master_key_stops_serve_before_any_listener_and_key_check_exits_2_key_check_mismatch()
 {
    // Arrange: the first start wrote the key check; a file's blob is on the volume.
    let mut pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac72");
    let file = File::new("sealed.txt", 4000, 72);
    let (node, _) = pipeline.publish_small(folder, &file, None);
    pipeline.stop();
    let key = pipeline.master_key_file();
    let right = fs::read(&key).expect("the master key");
    fs::write(&key, [0xa5u8; 32]).expect("swap the master key");

    // Act
    let refused = pipeline.stack.refused();
    let checked = pipeline.command(&["key-check"]);

    // Assert
    assert_eq!(
        refused.exited.code,
        Some(2),
        "{}",
        refused.exited.stderr_text()
    );
    assert!(!refused.accepted, "a listener accepted a connection");
    assert!(refused.exited.stderr_text().contains("KeyCheckMismatch"));
    assert_eq!(checked.code, Some(2), "{checked:?}");
    assert_eq!(
        checked.envelope()["code"],
        "KeyCheckMismatch",
        "{checked:?}"
    );
    fs::write(&key, right).expect("restore the master key");
    pipeline.restart();
    assert_eq!(pipeline.download(node), file.bytes);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn f14_step6_keycheck_init_writes_the_key_check_of_a_volume_without_one_only_for_the_confirmed_key()
 {
    // Arrange: a volume back from a backup without `.keycheck`, its blobs present.
    let mut pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("step6");
    let file = File::new("kept.txt", 4000, 66);
    let (node, _) = pipeline.publish_small(folder, &file, None);
    pipeline.stop();
    fs::remove_file(pipeline.key_check_file()).expect("drop the key check");
    let key = pipeline.master_key_file();
    let right = fs::read(&key).expect("the master key");

    // Act
    let refused = pipeline.stack.refused();
    fs::write(&key, [0xa5u8; 32]).expect("swap the master key");
    let wrong = pipeline.command(&["admin", "keycheck-init"]);
    let absent_after_wrong = !pipeline.key_check_file().exists();
    fs::write(&key, right).expect("restore the master key");
    let confirmed = pipeline.command(&["admin", "keycheck-init"]);
    let checked = pipeline.command(&["key-check"]);

    // Assert
    assert_eq!(
        refused.exited.code,
        Some(2),
        "{}",
        refused.exited.stderr_text()
    );
    assert!(!refused.accepted, "a listener accepted a connection");
    assert!(
        refused
            .exited
            .stderr_text()
            .contains("KeyCheckAbsentButBlobsExist")
    );
    assert_eq!(wrong.code, Some(2), "{wrong:?}");
    assert_eq!(
        wrong.envelope()["code"],
        "KeyCheckSampleFailed",
        "{wrong:?}"
    );
    assert!(
        absent_after_wrong,
        "a refused candidate wrote the key check"
    );
    assert_eq!(confirmed.code, Some(0), "{confirmed:?}");
    assert!(pipeline.key_check_file().is_file());
    assert_eq!(checked.code, Some(0), "{checked:?}");
    pipeline.restart();
    assert_eq!(pipeline.download(node), file.bytes);
    pipeline.finish().await;
}

#[test]
fn ac4_every_f14_operation_has_a_production_caller_and_serve_checks_before_its_listeners() {
    // Arrange
    let callers: &[Caller] = &[
        (
            "EpochAdmin::bump_epoch, then the restore-tagged checks (admin epoch-bump)",
            "server/src/maintenance_commands.rs",
            &[
                "admin.bump_epoch(clock.now_utc())",
                "CheckSelection::Tagged(DoctorTag::Restore)",
            ],
        ),
        (
            "the commands' dispatch",
            "server/src/dispatch.rs",
            &[
                "maintenance_commands::epoch_bump(",
                "key_commands::key_check(",
                "key_commands::keycheck_init(",
            ],
        ),
        (
            "EpochAdmin::marker_status and MasterKeyring::load_and_verify (serve)",
            "server/src/serve/start.rs",
            &["epoch.marker_status()", "keyring.load_and_verify()"],
        ),
        (
            "EpochMarker::write after the commit (05)",
            "components/05_sync_feed/src/epoch_repo.rs",
            &["impl EpochAdmin for PgEpochAdmin", ".write(epoch)"],
        ),
        (
            "MasterKeyring::{load_and_verify, init_key_check} (key-check, admin keycheck-init)",
            "server/src/key_commands.rs",
            &[
                "keyring.load_and_verify()",
                ".init_key_check(&MasterKey(bytes))",
            ],
        ),
        (
            "the missing-blob report: 04's inventory pages joined with 02's describe_blob_revisions, tagged restore",
            "server/src/missing_blobs_check.rs",
            &[
                "self.inventory.scan_page(",
                "self.tree.describe_blob_revisions(",
                "&[DoctorTag::Restore]",
            ],
        ),
        (
            "the missing-blob report registered by 12",
            "server/src/root/lists.rs",
            &["Arc::new(MissingBlobsReport::new("],
        ),
    ];
    let start = production_source("server/src/serve/start.rs");
    let order = [
        "schema_check(&db)",
        "keyring.load_and_verify()",
        "epoch.marker_status()",
        ".bind()",
    ];

    // Act
    let missing = missing_callers(callers);
    let at: Vec<Option<usize>> = order.iter().map(|needle| start.find(needle)).collect();

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
    assert!(at.iter().all(Option::is_some), "{order:?} at {at:?}");
    assert!(
        at.windows(2).all(|pair| pair[0] < pair[1]),
        "{order:?} at {at:?}"
    );
}
