//! AZ-3873 AC-3 to AC-5: the silence and wall watchdogs, the unhealthy slot and its recovery through the gate.

mod api_support;
mod slot_support;

use std::time::{Duration, Instant};

use media_shared::decoder_protocol::Request;
use media_shared::types::{Disposition, FailureReport, JobKind};
use sc_observability::{ReadinessCheck, ReadinessItem, ReadinessStatus};
use slot_support::{
    Answer, Entry, Rig, ScriptedApi, calls, json, photo_probe, wait_until, watchdog_config,
};
use sunnycloud_worker::{JobEnd, SlotName, SlotStep};

const WORKER: &str = "worker-watchdog";

fn retry(code: &str) -> JobEnd {
    JobEnd::Failed(FailureReport {
        code: code.to_owned(),
        disposition: Disposition::Retry,
    })
}

async fn photo_rig(silence: u64, grace: u64, wall: u64, stills: Answer) -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    let config = watchdog_config(
        Duration::from_millis(silence),
        Duration::from_millis(grace),
        Duration::from_millis(wall),
    );
    let rig = Rig::with_config(&api, SlotName::Interactive, WORKER, config);
    rig.decoder
        .probes_fall_back_to(Answer::Probe(photo_probe()));
    rig.decoder.stills_fall_back_to(stills);
    (api, rig)
}

async fn readiness(rig: &Rig) -> ReadinessItem {
    rig.slot_loop.readiness().check().await
}

fn render_arrival(rig: &Rig) -> Instant {
    rig.decoder
        .seen()
        .iter()
        .find(|seen| matches!(seen.request, Request::RenderStill { .. }))
        .map(|seen| seen.at)
        .expect("the render request arrived")
}

/// Runs one step; once the render request is in, the decoder starts failing its `self_check`, so the slot can
/// not return to service after the watchdog has taken it out.
async fn step_then_break_the_gate(rig: &mut Rig) -> SlotStep {
    let (step, ()) = tokio::join!(rig.slot_loop.step(), async {
        wait_until(|| rig.decoder.requests().len() == 2).await;
        rig.decoder.fail_self_checks();
    });
    step
}

#[tokio::test]
async fn a_silent_decoder_is_cancelled_then_the_slot_stops_claiming_and_the_job_is_retried() {
    // Arrange
    let (api, mut rig) = photo_rig(300, 100, 60_000, Answer::Silent).await;

    // Act
    let step = step_then_break_the_gate(&mut rig).await;
    let ready = readiness(&rig).await;
    let blocked = tokio::time::timeout(Duration::from_millis(300), rig.slot_loop.step()).await;
    let requests = api.peer.requests();

    // Assert
    let cancels = rig.decoder.cancels();
    let cancel_after = cancels[0] - render_arrival(&rig);
    assert_eq!(step, SlotStep::Ran(retry("DecoderSilent")));
    assert_eq!(cancels.len(), 1);
    assert!(
        cancel_after >= Duration::from_millis(300) && cancel_after < Duration::from_millis(450),
        "cancel after {cancel_after:?}"
    );
    assert_eq!(ready.status, ReadinessStatus::Failing);
    assert!(blocked.is_err(), "the slot claimed while unhealthy");
    assert_eq!(calls(&requests), ["claim image", "input", "failure"]);
    assert_eq!(json(&requests[2])["code"], "DecoderSilent");
    assert_eq!(json(&requests[2])["disposition"], "retry");
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn a_decoder_that_only_sends_alive_is_cancelled_at_the_wall_limit_with_the_same_outcome() {
    // Arrange
    let answer = Answer::AliveOnly(Duration::from_millis(50));
    let (api, mut rig) = photo_rig(10_000, 100, 600, answer).await;

    // Act
    let step = step_then_break_the_gate(&mut rig).await;
    let ready = readiness(&rig).await;
    let requests = api.peer.requests();

    // Assert
    let cancels = rig.decoder.cancels();
    let cancel_after = cancels[0] - render_arrival(&rig);
    assert_eq!(step, SlotStep::Ran(retry("WallLimit")));
    assert!(
        cancel_after >= Duration::from_millis(600) && cancel_after < Duration::from_millis(750),
        "cancel after {cancel_after:?}"
    );
    assert_eq!(ready.status, ReadinessStatus::Failing);
    assert_eq!(calls(&requests), ["claim image", "input", "failure"]);
    assert_eq!(json(&requests[2])["code"], "WallLimit");
}

#[tokio::test]
async fn a_decoder_that_answers_inside_the_silence_timeout_is_left_alone() {
    // Arrange
    let (api, mut rig) = photo_rig(
        400,
        100,
        60_000,
        Answer::Image {
            width: 1440,
            height: 1080,
        },
    )
    .await;
    rig.decoder.delay(Duration::from_millis(150));

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    assert!(rig.decoder.cancels().is_empty());
    assert!(readiness(&rig).await.status == ReadinessStatus::Ok);
    assert!(!calls(&api.peer.requests()).contains(&"failure".to_owned()));
}

#[tokio::test]
async fn a_hostile_frame_fails_the_job_and_the_slot_claims_again_only_after_the_gate_passes() {
    // Arrange
    let (api, mut rig) = photo_rig(
        20_000,
        5_000,
        60_000,
        Answer::Image {
            width: 1440,
            height: 1080,
        },
    )
    .await;
    rig.decoder.stills([Answer::Garbage(vec![0xFF; 4])]);
    api.enqueue(Entry::new(2, JobKind::Stills));

    // Act
    let first = rig.run_one().await;
    let after_violation = readiness(&rig).await;
    let hellos_before = rig.decoder.hellos();
    let second = rig.run_one().await;
    let after_gate = readiness(&rig).await;

    // Assert
    assert_eq!(first, retry("ProtocolViolation"));
    assert_eq!(after_violation.status, ReadinessStatus::Failing);
    assert_eq!(after_violation.detail, "unhealthy: protocol violation");
    assert_eq!(hellos_before, 1);
    assert_eq!(second, JobEnd::Derived);
    assert_eq!(rig.decoder.hellos(), 2);
    assert_eq!(rig.decoder_metrics.restarts(SlotName::Interactive), 1);
    assert_eq!(after_gate.status, ReadinessStatus::Ok);
    assert_eq!(rig.scratch_entries(), 0);
}
