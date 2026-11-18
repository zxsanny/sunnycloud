//! AZ-3667 (E-22/p14), flow F15 through the real `sunnycloud` binary on PostgreSQL 18 and a blob volume, over HTTP
//! with real uploads: clean runs of the scheduled cycle and `scrub run --full` (AC-1), a flipped byte and a removed
//! file reported as findings, audit events, health and a clean download failure (AC-2), the scrub metric of the
//! API process (AC-2), one `scrub_summary` per run (AC-3), the scheduled scrub pausing while the activity gauge is
//! above `scrub_pause_active_streams` (AC-4), the production callers (AC-5) and a full scrub in the API process and
//! from the CLI beside uploads in progress and the GC (AC-80).
//!
//! Corruption is seeded on the volume from outside. Durations are short config (`scrub_interval`, `gc_interval`,
//! `retention`) and every wait polls. `scrub run` is a separate process with no listener, so the scrub metric is
//! read from the API process's own scheduled cycles; the CLI's outcome is its exit code, its counts, the audit log
//! and the blob rows.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::time::{Duration, Instant};

use pipeline_support::Pipeline;
use pipeline_support::gc::Stored as Rows;
use pipeline_support::scrub::Stored;
use pipeline_support::wiring::{Caller, missing_callers};
use uuid::Uuid;

const BOUND: Duration = Duration::from_secs(60);
const SCHEDULER_TASK: &str = "scrub_janitor";

fn deadline() -> Instant {
    Instant::now() + BOUND
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

/// Three small files, a photo with its two derived blobs and a two-block file; the committed blob count is 7.
async fn volume(pipeline: &Pipeline) -> (Vec<Stored>, [Uuid; 2]) {
    let docs = pipeline.folder("docs");
    let mut files = Vec::new();
    for (index, size) in [3000, 70_000, 1_500_000].into_iter().enumerate() {
        let seed = 10 + index as u64;
        files.push(
            pipeline
                .small(docs, &format!("f{index}.bin"), size, seed)
                .await,
        );
    }
    let (photo, derived) = pipeline.photo_with_derived(docs, "cat.jpg", 5).await;
    files.push(photo);
    files.push(pipeline.large(docs, "big.bin", 2, 6).await);
    (files, derived)
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_the_scheduled_cycle_and_a_full_cli_run_find_nothing_and_status_shows_a_clean_volume() {
    // Arrange
    let pipeline = Pipeline::start(&[("scrub_interval", "6s")]).await;
    volume(&pipeline).await;
    pipeline.make_admin().await;
    let blobs = pipeline.committed_blobs().await;

    // Act
    pipeline
        .poll(
            "the scheduled cycle did not scrub every blob",
            deadline(),
            async || {
                pipeline.janitor_cycles(SCHEDULER_TASK) >= 1 && pipeline.never_scrubbed().await == 0
            },
        )
        .await;
    let scheduled = pipeline.summaries().await;
    let full = pipeline.scrub_run(true);
    let status = pipeline.scrub_status();

    // Assert
    assert_eq!(blobs.len(), 7);
    let scheduled_checked: u64 = scheduled
        .iter()
        .map(|s| s["checked"].as_u64().unwrap_or(0))
        .sum();
    assert!(scheduled_checked >= 7, "{scheduled:?}");
    assert!(
        scheduled
            .iter()
            .all(|s| s["mismatched"] == 0 && s["missing"] == 0),
        "{scheduled:?}"
    );
    assert_eq!(full.code, Some(0), "{full:?}");
    assert_eq!(
        (
            full.count("checked"),
            full.count("mismatched"),
            full.count("missing")
        ),
        (7, 0, 0)
    );
    assert!(status["oldest_scrubbed_at_micros"].is_i64(), "{status}");
    assert_eq!(
        (&status["unscrubbed"], &status["unavailable"]),
        (&0.into(), &0.into())
    );
    assert!(pipeline.findings().await.is_empty());
    assert!(pipeline.unavailable_blobs().await.is_empty());
    assert_eq!(pipeline.blobstore_health().0, "ok");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_a_flipped_byte_and_a_removed_file_exit_2_with_two_findings_degraded_health_and_a_clean_download_failure()
 {
    // Arrange: one corrupt original, one missing derived blob.
    let pipeline = Pipeline::start(&[]).await;
    let (files, derived) = volume(&pipeline).await;
    pipeline.make_admin().await;
    let (corrupt, missing) = (&files[0], derived[0]);
    pipeline.corrupt(corrupt.blob);
    pipeline.remove_file(missing);

    // Act
    let run = pipeline.scrub_run(true);
    let download = pipeline.call(
        "GET",
        &format!("/api/v1/nodes/{}/content", corrupt.node),
        &[],
        &[],
    );
    let healthy = pipeline.download(files[1].node);
    let (health, detail) = pipeline.blobstore_health();
    let status = pipeline.scrub_status();

    // Assert
    assert_eq!(run.code, Some(2), "{run:?}");
    assert_eq!(
        (run.count("mismatched"), run.count("missing")),
        (1, 1),
        "{run:?}"
    );
    let mut want = vec![
        (corrupt.blob, "corrupt/scrub".to_owned()),
        (missing, "missing/scrub".to_owned()),
    ];
    want.sort();
    assert_eq!(
        pipeline.finding_pairs().await,
        want,
        "no extra event after the failed download"
    );
    assert_eq!(
        pipeline.unavailable_blobs().await,
        sorted(vec![corrupt.blob, missing])
    );
    assert_eq!(
        (health.as_str(), detail.as_str()),
        ("degraded", "2 blobs unavailable")
    );
    assert_eq!(status["unavailable"], 2);
    assert_eq!(download.status, 404, "{download:?}");
    assert_eq!(download.code(), "content_unavailable");
    let head = &corrupt.bytes[..64];
    assert!(
        !download.body.windows(64).any(|w| w == head),
        "plaintext served"
    );
    assert_eq!(healthy, files[1].bytes);
    let summaries = pipeline.summaries().await;
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        (&summaries[0]["mismatched"], &summaries[0]["missing"]),
        (&1.into(), &1.into())
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_the_scheduled_scrub_raises_its_metrics_and_degrades_health() {
    // Arrange: the API process's own cycles every 3 s; the corruption is seeded from outside.
    let pipeline = Pipeline::start(&[("scrub_interval", "3s")]).await;
    let (files, derived) = volume(&pipeline).await;
    pipeline.make_admin().await;
    let before = (
        pipeline.metric("scrub_mismatches_total"),
        pipeline.metric("scrub_missing_total"),
    );
    pipeline.corrupt(files[1].blob);
    pipeline.remove_file(derived[1]);

    // Act
    pipeline
        .poll(
            "the scheduled scrub did not report both findings",
            deadline(),
            async || {
                pipeline.metric("scrub_mismatches_total") >= 1.0
                    && pipeline.metric("scrub_missing_total") >= 1.0
            },
        )
        .await;
    let (health, detail) = pipeline.blobstore_health();

    // Assert
    assert_eq!(before, (0.0, 0.0));
    let mut want = vec![
        (files[1].blob, "corrupt/scrub".to_owned()),
        (derived[1], "missing/scrub".to_owned()),
    ];
    want.sort();
    assert_eq!(pipeline.finding_pairs().await, want);
    assert_eq!(
        (health.as_str(), detail.as_str()),
        ("degraded", "2 blobs unavailable")
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_every_scheduled_cycle_and_every_cli_run_leaves_exactly_one_summary_with_integer_counts()
 {
    // Arrange
    let pipeline = Pipeline::start(&[("scrub_interval", "4s")]).await;
    volume(&pipeline).await;

    // Act: a full and a due run from the CLI between the scheduled cycles.
    pipeline
        .poll("no scheduled cycle finished", deadline(), async || {
            pipeline.janitor_cycles(SCHEDULER_TASK) >= 1
        })
        .await;
    let cli = [pipeline.scrub_run(true), pipeline.scrub_run(false)];
    let settled = deadline();
    let summaries = loop {
        let (before, rows, after) = (
            pipeline.janitor_cycles(SCHEDULER_TASK),
            pipeline.summaries().await,
            pipeline.janitor_cycles(SCHEDULER_TASK),
        );
        if before == after && rows.len() as u64 == before + 2 {
            break rows;
        }
        assert!(
            Instant::now() < settled,
            "{before}/{after} cycles, {} summaries",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    // Assert
    assert_eq!((cli[0].code, cli[1].code), (Some(0), Some(0)));
    for summary in &summaries {
        for key in ["checked", "mismatched", "missing", "duration_ms"] {
            assert!(summary[key].is_u64(), "{key} in {summary}");
        }
    }
    assert!(
        summaries
            .iter()
            .any(|s| s["checked"] == cli[0].count("checked")),
        "{summaries:?}"
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_the_scheduled_scrub_reads_nothing_while_downloads_hold_the_gauge_and_finishes_after_they_end()
 {
    // Arrange: pause above 1 stream; two unread downloads of a 32 MiB file, opened before the first cycle at 15 s.
    let pipeline = Pipeline::start(&[
        ("scrub_interval", "15s"),
        ("scrub_pause_active_streams", "1"),
    ])
    .await;
    let docs = pipeline.folder("docs");
    pipeline.small(docs, "small.bin", 3000, 1).await;
    let big = pipeline.large(docs, "big.bin", 16, 2).await;
    let blobs = pipeline.committed_blobs().await.len();
    let held = [
        pipeline.hold_download(big.node),
        pipeline.hold_download(big.node),
    ];

    // Act
    pipeline
        .poll("the scheduled run did not start", deadline(), async || {
            pipeline.metric("scrub_running") == 1.0
        })
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let while_held = (
        pipeline.metric("scrub_blobs_verified_total"),
        pipeline.metric("scrub_running"),
        pipeline.janitor_cycles(SCHEDULER_TASK),
        pipeline.never_scrubbed().await,
    );
    drop(held);
    pipeline
        .poll(
            "the cycle did not finish after the downloads ended",
            deadline(),
            async || pipeline.janitor_cycles(SCHEDULER_TASK) >= 1,
        )
        .await;

    // Assert
    assert_eq!(blobs, 2);
    assert_eq!(while_held, (0.0, 1.0, 0, 2));
    assert_eq!(pipeline.metric("scrub_blobs_verified_total"), 2.0);
    assert_eq!(pipeline.metric("scrub_running"), 0.0);
    assert_eq!(pipeline.never_scrubbed().await, 0);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac80_full_scrubs_beside_uploads_in_progress_and_the_gc_report_only_the_seeded_corruptions()
{
    // Arrange: the API scrubs every second, the GC runs every second and collects 2 s after a deletion.
    let pipeline = Pipeline::start(&[
        ("scrub_interval", "1s"),
        ("gc_interval", "1s"),
        ("retention", "2s"),
    ])
    .await;
    let docs = pipeline.folder("docs");
    let mut keepers = Vec::new();
    for index in 0..4 {
        let name = format!("keep{index}.bin");
        keepers.push(
            pipeline
                .small(docs, &name, 3000 + 500 * index, index as u64)
                .await,
        );
    }
    let (corrupt, missing) = (keepers[0].blob, keepers[1].blob);
    pipeline.corrupt(corrupt);
    pipeline.remove_file(missing);
    let inflight = pipeline.inflight(docs, 2).await;
    let mut queued: Vec<Uuid> = Vec::new();
    let mut previous: Option<Stored> = None;

    // Act: rounds of a new upload, a deletion queuing the previous file for the GC, and a full CLI scrub.
    let (mut clis, mut beside_gc, mut removed) = (Vec::new(), 0, 0);
    let end = deadline();
    for round in 0u64.. {
        let name = format!("churn{round}.bin");
        let fresh = pipeline
            .small(docs, &name, 2000 + round as usize, 100 + round)
            .await;
        if let Some(old) = previous.replace(fresh) {
            assert!((200..300).contains(&pipeline.delete_file(old.node).status));
            queued.push(old.blob);
        }
        let pending = pipeline.not_collected(&queued).await;
        clis.push(pipeline.scrub_run(true));
        beside_gc += usize::from(pending > 0);
        removed = queued.len() - pending;
        if round >= 5 && removed > 0 {
            break;
        }
        assert!(
            Instant::now() < end,
            "the GC removed nothing in {round} rounds"
        );
    }
    pipeline.wait_stored(&queued, Rows::GONE, deadline()).await;
    pipeline
        .poll("fewer than 3 API cycles", deadline(), async || {
            pipeline.janitor_cycles(SCHEDULER_TASK) >= 3
        })
        .await;
    let mut finished = Vec::new();
    for (upload, file, _) in &inflight {
        finished.push((pipeline.finish_inflight(docs, *upload, file).await, file));
    }

    // Assert
    let states: Vec<&Option<String>> = inflight.iter().map(|(_, _, state)| state).collect();
    assert_eq!(states, [&Some("uploading".to_owned()); 2]);
    assert!(
        removed > 0 && beside_gc >= 2,
        "removed {removed}, CLI runs beside the GC {beside_gc}"
    );
    for run in &clis {
        assert_eq!(run.code, Some(2), "{run:?}");
        assert_eq!(
            (run.count("mismatched"), run.count("missing")),
            (1, 1),
            "{run:?}"
        );
    }
    let mut want = vec![
        (corrupt, "corrupt/scrub".to_owned()),
        (missing, "missing/scrub".to_owned()),
    ];
    want.sort();
    assert_eq!(pipeline.finding_pairs().await, want);
    assert_eq!(
        pipeline.unavailable_blobs().await,
        sorted(vec![corrupt, missing])
    );
    for summary in pipeline.summaries().await {
        assert!(
            summary["mismatched"].as_u64() <= Some(1) && summary["missing"].as_u64() <= Some(1),
            "{summary}"
        );
    }
    for (node, file) in finished {
        assert_eq!(
            pipeline.download(node),
            file.bytes,
            "an in-flight upload was disturbed"
        );
    }
    pipeline.finish().await;
}

#[test]
fn ac5_every_f15_operation_has_a_production_caller() {
    // Arrange
    let callers: &[Caller] = &[
        (
            "Scrubber::{start_run, run_step, finish_run, status} from the scrub commands",
            "server/src/scrub_commands.rs",
            &[
                "scrubber.start_run(mode, rate)",
                "scrubber.run_step(run)",
                "scrubber.finish_run(&run)",
                ".status()",
            ],
        ),
        (
            "the Scrubber service the commands ask for",
            "server/src/root/storage.rs",
            &["register::<dyn Scrubber"],
        ),
        (
            "the scheduler registration of ScrubJanitor",
            "server/src/root/lists.rs",
            &[
                "Arc::new(ScrubJanitor::new(&blobs.config, blobs.scrubber.clone()))",
                "blobs.config.scrub_interval",
            ],
        ),
        (
            "ScrubJanitor's cycle: a Due run, stepped under the budget, then finished",
            "components/04_blobstore/src/scrub/janitor.rs",
            &[
                "start_run_at(ScrubMode::Due",
                "step_until(&run, Some(deadline))",
                "finish_run(&run)",
            ],
        ),
        (
            "BlobHealth as a HealthContributor in 07's aggregation list",
            "server/src/root/lists.rs",
            &["BlobHealthContributor::new(blobs.health.clone())"],
        ),
        (
            "the activity gauge fed by the writer, the reader and read by the scrubber",
            "server/src/root/storage.rs",
            &[
                ".with_activity(activity.clone())",
                "let activity = ActivityGauge::new();",
            ],
        ),
        (
            "block writes and open downloads entering the gauge",
            "components/04_blobstore/src/writer/mod.rs",
            &["ActivityGauge::enter"],
        ),
        (
            "open downloads entering the gauge",
            "components/04_blobstore/src/reader/mod.rs",
            &["ActivityGauge::enter"],
        ),
    ];

    // Act
    let missing_text = missing_callers(callers);

    // Assert
    assert!(missing_text.is_empty(), "{missing_text:#?}");
}
