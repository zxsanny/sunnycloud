//! AZ-3874 AC-1, AC-3 and AC-4 on the whole supervisor (`run_worker`) with short settings, a manual clock, a
//! scripted API listener and two fake decoders.

mod api_support;
mod slot_support;
mod supervisor_support;
mod support;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use media_shared::types::JobKind;
use slot_support::{Answer, Entry, compliant_probe, photo_probe, wait_until};
use supervisor_support::{Harness, Timing, body, is_status};

fn plant_leftovers(root: &Path) {
    let leftover = root.join("leftover-job");
    fs::create_dir_all(leftover.join("in")).expect("leftover tree");
    fs::write(leftover.join("in/source"), b"old").expect("leftover file");
}

fn script_photos(harness: &Harness) {
    for decoder in [&harness.interactive, &harness.general] {
        decoder
            .probes_fall_back_to(Answer::Probe(photo_probe()))
            .stills_fall_back_to(Answer::Image {
                width: 320,
                height: 240,
            });
    }
}

#[tokio::test]
async fn ac1_the_roots_are_swept_before_the_first_claim_and_only_the_ready_slot_claims() {
    // Arrange
    let mut harness = Harness::new(&Timing::default()).await;
    script_photos(&harness);
    harness.interactive.fail_self_checks();
    harness.api.enqueue(Entry::new(1, JobKind::Stills));
    plant_leftovers(&harness.interactive_root);
    plant_leftovers(&harness.general_root);

    // Act
    let running = harness.start();
    harness
        .wait_for_request(|request| request.path.ends_with("/claims"))
        .await;
    wait_until(|| !harness.general.requests().is_empty()).await;
    let (readyz, ready_body) = harness.get("/readyz").await;
    let ended = running.stop().await;

    // Assert
    assert!(!harness.interactive_root.join("leftover-job").exists());
    assert!(!harness.general_root.join("leftover-job").exists());
    assert!(
        harness.interactive.requests().is_empty(),
        "the interactive slot took a job: {:?}",
        harness.interactive.requests()
    );
    assert_eq!(readyz, 503, "{ready_body}");
    assert!(ready_body.contains("decoder_interactive"), "{ready_body}");
    assert!(ended.is_ok(), "{ended:?}");
}

#[tokio::test]
async fn ac3_a_status_with_the_image_age_is_sent_every_interval() {
    // Arrange
    let mut harness = Harness::new(&Timing::default()).await;

    // Act
    let running = harness.start();
    harness.wait_for_statuses(4).await;
    let ended = running.stop().await;

    // Assert
    // The report at start-up may come before the gates pass; every later one is healthy.
    let statuses = harness.statuses();
    for status in &statuses[1..] {
        assert_eq!(body(status)["decoder_ok"], true);
        assert_eq!(body(status)["image_age_days"], 20);
    }
    let gaps: Vec<Duration> = statuses.windows(2).map(|w| w[1].at - w[0].at).collect();
    assert!(
        gaps.iter()
            .all(|gap| *gap >= Duration::from_millis(100) && *gap <= Duration::from_millis(800)),
        "gaps: {gaps:?}"
    );
    assert!(ended.is_ok(), "{ended:?}");
}

#[tokio::test]
async fn ac3_a_failing_gate_reports_decoder_ok_false() {
    // Arrange
    let mut harness = Harness::new(&Timing::default()).await;
    harness.general.fail_self_checks();

    // Act
    let running = harness.start();
    harness.wait_for_statuses(2).await;
    let ended = running.stop().await;

    // Assert
    let statuses = harness.statuses();
    assert!(statuses.iter().all(|s| body(s)["decoder_ok"] == false));
    assert!(ended.is_ok(), "{ended:?}");
}

#[tokio::test]
async fn ac3_a_transient_status_failure_is_followed_by_the_next_report() {
    // Arrange
    let mut harness = Harness::new(&Timing::default()).await;
    harness.api.status_replies([500, 503]);

    // Act
    let running = harness.start();
    harness.wait_for_statuses(4).await;
    let ended = running.stop().await;

    // Assert
    assert!(ended.is_ok(), "{ended:?}");
}

#[tokio::test]
async fn ac3_after_a_403_the_reports_stop_and_the_worker_is_not_ready() {
    // Arrange
    let mut harness = Harness::new(&Timing::default()).await;
    harness.api.status_replies([403]);

    // Act
    let running = harness.start();
    harness.wait_for_statuses(1).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let sent = harness.statuses().len();
    let (readyz, ready_body) = harness.get("/readyz").await;
    let ended = running.stop().await;

    // Assert
    assert_eq!(sent, 1, "reports after the 403");
    assert_eq!(readyz, 503, "{ready_body}");
    assert!(ready_body.contains("worker_api"), "{ready_body}");
    assert!(ended.is_ok(), "{ended:?}");
}

#[tokio::test]
async fn ac4_sigterm_stops_claims_finishes_the_short_job_and_cancels_and_releases_the_long_one() {
    // Arrange
    let timing = Timing::default();
    let mut harness = Harness::new(&timing).await;
    harness
        .general
        .probes_fall_back_to(Answer::Probe(compliant_probe(60_000)))
        .stills_fall_back_to(Answer::Image {
            width: 320,
            height: 240,
        })
        .delay(Duration::from_secs(30));
    harness
        .interactive
        .probes_fall_back_to(Answer::Probe(photo_probe()))
        .stills_fall_back_to(Answer::Image {
            width: 320,
            height: 240,
        })
        .delay(Duration::from_millis(100));
    harness.api.enqueue(Entry::new(1, JobKind::Poster));
    let running = harness.start();
    wait_until(|| harness.general.requests().len() >= 2).await;
    harness.api.enqueue(Entry::new(2, JobKind::Stills));
    wait_until(|| !harness.interactive.requests().is_empty()).await;

    // Act
    let stopped_at = Instant::now();
    let ended = running.ended_after_stop(Duration::from_secs(5)).await;
    harness.collect();

    // Assert
    assert!(ended.is_ok(), "{ended:?}");
    let late_claims = harness
        .seen
        .iter()
        .filter(|r| r.path.ends_with("/claims") && r.at > stopped_at)
        .count();
    let posted = |job: u32, kind: &str| {
        harness
            .seen
            .iter()
            .any(|r| r.method == "PUT" && r.path.ends_with(&format!("{job:012}/outputs/{kind}")))
    };
    let released = harness
        .seen
        .iter()
        .any(|r| r.method == "DELETE" && r.path.ends_with("000000000001/lease"));
    assert_eq!(late_claims, 0);
    assert!(posted(2, "preview") && posted(2, "thumb"));
    assert!(!posted(1, "thumb"));
    assert!(released, "no release of the long job");
    let cancels = harness.general.cancels();
    assert_eq!(cancels.len(), 1);
    assert!(
        cancels[0] - stopped_at >= timing.shutdown_grace - Duration::from_millis(50),
        "cancelled before the grace passed"
    );
    assert!(harness.statuses().iter().all(|r| is_status(r)));
}
