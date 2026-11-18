//! AZ-3663 (E-22/p11), flow F12 through the real `sunnycloud` binary (`--features test-clock`) on PostgreSQL 18 and
//! a blob volume: retention then removal by the scheduler's GC cycle, and `admin gc-run` (AC-1), derived blobs
//! following their content (AC-2), `admin restore` (AC-3), the tombstone purge raising the horizon (AC-4), a crash
//! between the unlink and the row removal (AC-5) and the production callers (AC-6).
//!
//! Time: the production durations (`retention` 7 d, `tombstone_horizon` 90 d) move through the test-clock offset
//! of the serving process (`POST /test/clock`); the janitor intervals are short config. `admin gc-run` and
//! `admin restore` are separate processes with their own clock at offset 0, so the scheduler cannot share a moved
//! clock with them: AC-1's `gc-run` case and AC-3 use a short `retention` on real time and poll
//! (`_docs/02_document/tests/environment.md`).
//!
//! AC-5: 04's sweeper parks a due file (renames it to `<file>.gc`, the unlink from the blob's name) and then, in a
//! second transaction, runs the owner cleanups and removes the rows. The test holds `FOR UPDATE` on the blob's
//! revision row from a superuser connection, so 02's cleanup (`DELETE FROM revisions`) waits on it: the file is
//! parked, the rows are all there and the sweeper's backend waits on the lock. The binary is killed (SIGKILL) in
//! that window, the lock released (the dead client's transaction rolls back), and a new binary is started.

#![cfg(feature = "test-clock")]

mod pipeline_support;
mod root_support;
mod serve_support;

use std::time::{Duration, Instant};

use pipeline_support::feed::split;
use pipeline_support::gc::{DAY, Stored, release};
use pipeline_support::media::jpeg_bytes;
use pipeline_support::tree::{feed_nodes, names};
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, uuid_of};
use serde_json::Value;
use uuid::Uuid;

const BOUND: Duration = Duration::from_secs(30);
const GC_EVERY_SECOND: (&str, &str) = ("gc_interval", "1s");
const GC_TASK: &str = "gc_sweeper";

fn deadline() -> Instant {
    Instant::now() + BOUND
}

/// A file deleted and a file overwritten, each queuing its original blob at now plus `retention`; the two blobs.
async fn deleted_and_replaced(pipeline: &Pipeline) -> [Uuid; 2] {
    let docs = pipeline.folder("docs");
    let (gone, gone_rev) = pipeline.publish_small(docs, &File::new("gone.txt", 3000, 1), None);
    let (_, old_rev) = pipeline.publish_small(docs, &File::new("kept.txt", 3000, 2), None);
    let edit = File::new("kept.txt", 4000, 3);
    pipeline.publish_small(docs, &edit, Some(old_rev));
    let deleted = pipeline.delete_file(gone);
    assert!((200..300).contains(&deleted.status), "{deleted:?}");
    [
        pipeline.blob_of(gone_rev).await,
        pipeline.blob_of(old_rev).await,
    ]
}

/// `sunnycloud --json admin restore` of `path` at `at`.
fn restore(pipeline: &Pipeline, path: &str, at: &str) -> pipeline_support::restore::Run {
    let account = pipeline.account.id.to_string();
    pipeline.command(&[
        "admin",
        "restore",
        "--account",
        &account,
        "--path",
        path,
        "--at",
        at,
    ])
}

fn child<'a>(listing: &'a [Value], name: &str) -> &'a Value {
    listing
        .iter()
        .find(|node| node["name"] == name)
        .unwrap_or_else(|| panic!("no `{name}` in {listing:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_originals_stay_through_day_6_and_the_scheduler_collects_them_after_day_7() {
    // Arrange: production retention (7 d), the GC cycle every second.
    let pipeline = Pipeline::start(&[GC_EVERY_SECOND]).await;
    let blobs = deleted_and_replaced(&pipeline).await;
    let before = [
        pipeline.stored(blobs[0]).await,
        pipeline.stored(blobs[1]).await,
    ];

    // Act
    pipeline.advance_clock(6 * DAY);
    pipeline.wait_cycles(GC_TASK, 2, deadline()).await;
    let on_day_6 = [
        pipeline.stored(blobs[0]).await,
        pipeline.stored(blobs[1]).await,
    ];
    pipeline.advance_clock(DAY + 3600);
    pipeline.wait_stored(&blobs, Stored::GONE, deadline()).await;

    // Assert
    assert_eq!(before, [Stored::QUEUED; 2]);
    assert_eq!(on_day_6, [Stored::QUEUED; 2]);
    assert_eq!(pipeline.error_lines(), Vec::<String>::new());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_gc_run_collects_the_same_two_due_originals_and_reports_their_count() {
    // Arrange: `retention` 2 s on real time (the command's clock is never offset); the scheduler's GC is 14 min.
    let pipeline = Pipeline::start(&[("retention", "2s")]).await;
    let blobs = deleted_and_replaced(&pipeline).await;
    let before = [
        pipeline.stored(blobs[0]).await,
        pipeline.stored(blobs[1]).await,
    ];
    pipeline
        .wait_past(pipeline_support::now_micros() + 2_000_000)
        .await;

    // Act
    let run = pipeline.command(&["admin", "gc-run"]);

    // Assert
    assert_eq!(before, [Stored::QUEUED; 2]);
    assert_eq!(run.code, Some(0), "{run:?}");
    let data = &run.envelope()["data"];
    assert_eq!(
        (
            &data["removed"],
            &data["remaining_due"],
            &data["budget_spent"]
        ),
        (&Value::from(2), &Value::from(0), &Value::from(false)),
        "{data}"
    );
    pipeline
        .wait_stored(&blobs, Stored::GONE, Instant::now())
        .await;
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_derived_blobs_go_without_waiting_for_retention_once_their_content_is_collected() {
    // Arrange: a JPEG with its thumb and preview from the worker routes, then deleted.
    let pipeline = Pipeline::start(&[GC_EVERY_SECOND, ("derived_janitor_interval", "1s")]).await;
    let photos = pipeline.folder("photos");
    let jpeg = jpeg_bytes(40_000, 7);
    let committed = pipeline.commit_as(&pipeline.device, photos, "cat.jpg", &jpeg, None);
    assert_eq!(committed["status"], "committed", "{committed}");
    let (node, revision) = (
        uuid_of(&committed, "node_id"),
        uuid_of(&committed, "revision_id"),
    );
    let claimed = pipeline.claim_image("gc-worker");
    assert_eq!(claimed.status, 200, "{claimed:?}");
    let job = uuid_of(&claimed.json(), "job_id");
    for kind in ["preview", "thumb"] {
        let output = pipeline.put_output("gc-worker", job, kind, &jpeg_bytes(2000, 9));
        assert_eq!(output.status, 204, "{kind}: {output:?}");
    }
    let derived = pipeline.derived().await;
    let derived_blobs: Vec<Uuid> = derived.iter().map(|(_, blob)| *blob).collect();
    let files_before: Vec<bool> = derived_blobs
        .iter()
        .map(|blob| pipeline.blob_file(*blob).is_file())
        .collect();
    let deleted = pipeline.delete_file(node);
    assert!((200..300).contains(&deleted.status), "{deleted:?}");

    // Act: past the original's retention once; no further move for the derived blobs.
    pipeline.advance_clock(7 * DAY + 3600);
    pipeline.wait_revision_collected(revision, deadline()).await;
    let horizon = deadline();
    while !pipeline.derived().await.is_empty() {
        assert!(
            Instant::now() < horizon,
            "derived rows outlived their content"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    pipeline
        .wait_stored(&derived_blobs, Stored::GONE, deadline())
        .await;

    // Assert
    let kinds: Vec<&str> = derived.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["preview", "thumb"]);
    assert_eq!(files_before, [true, true]);
    assert!(pipeline.derived().await.is_empty());
    assert_eq!(pipeline.error_lines(), Vec::<String>::new());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_restore_brings_back_original_and_restored_placements_and_refuses_a_collected_blob() {
    // Arrange: `retention` 10 s on real time. `c.txt` is deleted and collected by `admin gc-run`; then `a.txt` is
    // deleted and the folder `old` holding `b.txt` is deleted.
    let pipeline = Pipeline::start(&[("retention", "10s")]).await;
    let (docs, old) = (pipeline.folder("docs"), pipeline.folder("old"));
    let a = File::new("a.txt", 3000, 11);
    let b = File::new("b.txt", 3100, 12);
    let (a_node, _) = pipeline.publish_small(docs, &a, None);
    pipeline.publish_small(old, &b, None);
    let (c_node, c_rev) = pipeline.publish_small(docs, &File::new("c.txt", 3200, 13), None);
    let before_c = pipeline.now_rfc3339();
    let c_blob = pipeline.blob_of(c_rev).await;
    assert!((200..300).contains(&pipeline.delete_file(c_node).status));
    let (_, c_due) = pipeline.gc_row(c_rev).await.expect("c.txt queued");
    pipeline.wait_past(c_due).await;
    let collected = pipeline.command(&["admin", "gc-run"]);
    assert_eq!(collected.code, Some(0), "{collected:?}");
    pipeline
        .wait_stored(&[c_blob], Stored::GONE, Instant::now())
        .await;
    let before_t = pipeline.now_rfc3339();
    let cursor = pipeline.feed_cursor();
    assert!((200..300).contains(&pipeline.delete_file(a_node).status));
    assert!((200..300).contains(&pipeline.delete_file(old).status));

    // Act
    let first = restore(&pipeline, "docs/a.txt", &before_t);
    let second = restore(&pipeline, "old/b.txt", &before_t);
    let head = pipeline.feed_cursor();
    let third = restore(&pipeline, "docs/c.txt", &before_c);

    // Assert
    assert_eq!(first.code, Some(0), "{first:?}");
    assert_eq!(first.envelope()["data"]["placement"], "original");
    assert_eq!(second.code, Some(0), "{second:?}");
    assert_eq!(second.envelope()["data"]["placement"], "restored");
    assert_eq!(names(&pipeline.children(docs)), ["a.txt".to_owned()].into());
    assert_eq!(pipeline.download(a_node), a.bytes);
    let restored = uuid_of(
        child(&pipeline.children(pipeline.account.root), "Restored"),
        "node_id",
    );
    let b_node = uuid_of(child(&pipeline.children(restored), "b.txt"), "node_id");
    assert_eq!(pipeline.download(b_node), b.bytes);
    let changes = pipeline.changes_since(&cursor);
    let live: Vec<Uuid> = changes
        .iter()
        .filter(|item| item["state"] == "live")
        .map(|item| uuid_of(item, "node_id"))
        .collect();
    assert!(
        live.contains(&a_node) && live.contains(&b_node),
        "{changes:#?}"
    );
    assert_eq!(third.code, Some(2), "{third:?}");
    assert_eq!(third.envelope()["code"], "beyond_retention");
    assert_eq!(
        pipeline.feed_cursor(),
        head,
        "the refused restore changed the feed"
    );
    assert!(!feed_nodes(&pipeline.changes_since(&head)).contains(&c_node));
    assert_eq!(pipeline.stored(c_blob).await, Stored::GONE);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_the_purge_of_a_90_day_tombstone_raises_the_horizon_and_an_older_cursor_gets_410() {
    // Arrange: the tree maintenance janitors every second; the default `tombstone_horizon` (90 d).
    let pipeline = Pipeline::start(&[("tree_maintenance_interval", "1s")]).await;
    let (node, _) =
        pipeline.publish_small(pipeline.account.root, &File::new("old.txt", 2000, 21), None);
    let below = pipeline.feed_cursor();
    assert!((200..300).contains(&pipeline.delete_file(node).status));
    let (_, tombstone_seq) = split(&pipeline.feed_cursor());
    let horizon_before = pipeline.purge_horizon().await;

    // Act
    pipeline.advance_clock(90 * DAY + 3600);
    let horizon = deadline();
    while pipeline.tombstone(node).await.is_some() {
        assert!(Instant::now() < horizon, "the tombstone was not purged");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let polled = pipeline.changes(&pipeline.device, &below, None, None);

    // Assert
    assert_eq!(horizon_before, 0);
    assert!(
        pipeline.purge_horizon().await >= i64::try_from(tombstone_seq).expect("a seq"),
        "the horizon was not raised to the tombstone"
    );
    assert_eq!(polled.status, 410, "{polled:?}");
    let problem = polled.json();
    assert_eq!(problem["code"], "resync_required", "{problem}");
    assert_eq!(problem["reason"], "below_purge_horizon", "{problem}");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac5_a_crash_after_the_unlink_and_before_the_row_removal_is_finished_by_the_next_cycle() {
    // Arrange: a deleted file's revision row locked from outside, then its blob made due.
    let mut pipeline = Pipeline::start(&[GC_EVERY_SECOND, ("gc_claim_lease", "1m")]).await;
    let (node, revision) = pipeline.publish_small(
        pipeline.account.root,
        &File::new("crash.txt", 5000, 31),
        None,
    );
    let blob = pipeline.blob_of(revision).await;
    assert!((200..300).contains(&pipeline.delete_file(node).status));
    let held = pipeline.lock_revision(revision).await;
    pipeline.advance_clock(7 * DAY + 3600);
    let window = deadline();
    while pipeline.revision_deletes_waiting().await == 0 {
        assert!(
            Instant::now() < window,
            "the sweeper never reached the row removal"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let in_window = (
        pipeline.parked_file(blob).is_file(),
        pipeline.stored(blob).await,
    );

    // Act
    pipeline.server.crash();
    release(held).await;
    pipeline.restart();
    let after_restart = pipeline.stored(blob).await;
    pipeline.advance_clock(8 * DAY);
    pipeline
        .wait_stored(&[blob], Stored::GONE, deadline())
        .await;

    // Assert
    let parked_rows = Stored {
        file: false,
        ..Stored::QUEUED
    };
    assert_eq!(in_window, (true, parked_rows));
    assert_eq!(after_restart, parked_rows, "the killed cycle removed rows");
    assert!(
        !pipeline.parked_file(blob).exists(),
        "the parked file stayed"
    );
    assert_eq!(pipeline.error_lines(), Vec::<String>::new());
    pipeline.finish().await;
}

#[test]
fn ac6_every_f12_operation_has_a_production_caller() {
    // Arrange
    let callers: &[Caller] = &[
        (
            "scheduler registrations of GcSweeper, OrphanSweeper, 02's purge janitor and DerivedJanitor",
            "server/src/root/lists.rs",
            &[
                "every(gc, blobs.config.gc_interval)",
                "every(Arc::new(orphans), blobs.config.orphan_sweep_interval)",
                "TreeJanitor::purge(",
                "Arc::new(DerivedJanitor::new(db.clone(), clock, blobs.gc.clone()))",
                ".start(jobs)",
            ],
        ),
        (
            "the BlobOwnerCleanup list (02, 08) given to 04's GcSweeper",
            "server/src/root/lists.rs",
            &[
                "Registered::new(\"02\", tree.lifecycle.clone())",
                "Registered::new(\"08\", media.lifecycle.clone())",
                "items(&cleanups.0)",
            ],
        ),
        (
            "the scheduler started by serve",
            "server/src/serve/start.rs",
            &["root::start_scheduler(services)"],
        ),
        (
            "FeedHorizon::raise_purge_horizon in 02's tombstone purge, with 05's horizon from 12",
            "components/02_tree/src/maintenance.rs",
            &[".raise_purge_horizon(tx, account, seq)"],
        ),
        (
            "05's horizon and 04's registry given to 02's maintenance and restore",
            "server/src/root/domain.rs",
            &["feed.horizon.clone()", "PgTreeRestore::new("],
        ),
        (
            "admin restore and admin gc-run",
            "server/src/maintenance_commands.rs",
            &[
                "restorer.restore_revision(account, target, at)",
                "services.get::<GcSweeper>()",
                ".run_cycle(clock.now_utc())",
            ],
        ),
        (
            "the commands' dispatch",
            "server/src/dispatch.rs",
            &[
                "maintenance_commands::restore(",
                "maintenance_commands::gc_run(",
            ],
        ),
    ];

    // Act
    let missing = missing_callers(callers);

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
}
