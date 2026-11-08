//! AZ-3872 AC-6: the reactions to 404, 409 and 403, and the empty-claim wait.

mod api_support;
mod slot_support;

use media_shared::decoder_protocol::DecodeError;
use media_shared::types::{Disposition, FailureReport, JobKind};
use slot_support::{Answer, Entry, Rig, ScriptedApi, calls, claimed_lanes, json, photo_probe};
use sunnycloud_worker::{JobEnd, SlotName, SlotStep};

const WORKER: &str = "worker-reactions";

async fn photo(entry: Entry) -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(entry);
    let rig = Rig::new(&api, SlotName::Interactive, WORKER);
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills_fall_back_to(Answer::Image {
        width: 1440,
        height: 1080,
    });
    (api, rig)
}

#[tokio::test]
async fn gone_input_closes_the_job_obsolete_with_no_decoder_call_and_no_retry() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).input_status(404)).await;

    // Act
    let end = rig.run_one().await;

    // Assert
    let requests = api.peer.requests();
    assert_eq!(end, JobEnd::Obsolete);
    assert_eq!(calls(&requests)[1..], ["input", "result"]);
    assert_eq!(json(&requests[2])["outcome"], "obsolete");
    assert!(rig.decoder.requests().is_empty());
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn gone_output_closes_the_job_obsolete() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).output_status("preview", 404)).await;

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Obsolete);
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output preview", "result"]
    );
}

#[tokio::test]
async fn lease_lost_on_the_input_sends_nothing_more_for_the_job() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).input_status(409)).await;

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::LeaseLost);
    assert_eq!(calls(&api.peer.requests())[1..], ["input"]);
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn lease_lost_on_an_output_sends_no_further_output_result_or_failure() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).output_status("preview", 409)).await;

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::LeaseLost);
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output preview"]
    );
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn lease_lost_on_the_failure_report_ends_quietly() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).failure_status(409)).await;
    rig.decoder
        .stills_fall_back_to(Answer::Error(DecodeError::Corrupt));

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::LeaseLost);
    assert_eq!(calls(&api.peer.requests())[1..], ["input", "failure"]);
}

#[tokio::test]
async fn an_unanswered_failure_report_is_still_the_documented_failure() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).failure_status(503)).await;
    rig.decoder
        .stills_fall_back_to(Answer::Error(DecodeError::Corrupt));

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(
        end,
        JobEnd::Failed(FailureReport {
            code: "Corrupt".to_owned(),
            disposition: Disposition::Permanent,
        })
    );
    assert_eq!(calls(&api.peer.requests())[1..], ["input", "failure"]);
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn forbidden_mid_job_marks_the_worker_not_ready_and_no_further_claim_is_sent() {
    // Arrange
    let (api, mut rig) = photo(Entry::new(1, JobKind::Stills).input_status(403)).await;
    api.enqueue(Entry::new(2, JobKind::Stills));

    // Act
    let end = rig.run_one().await;
    let next = rig.slot_loop.step().await;

    // Assert
    assert_eq!(end, JobEnd::Refused);
    assert_eq!(next, SlotStep::Stopped);
    assert!(rig.access.is_refused());
    assert_eq!(calls(&api.peer.requests()), ["claim image", "input"]);
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn forbidden_claim_stops_the_loop_after_one_request() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.refuse_claims(403);
    let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);

    // Act
    rig.slot_loop.run().await;

    // Assert
    assert!(rig.access.is_refused());
    assert_eq!(calls(&api.peer.requests()), ["claim image"]);
    assert_eq!(rig.slot_loop.step().await, SlotStep::Stopped);
    assert!(api.peer.requests().is_empty());
}

#[tokio::test]
async fn an_empty_interactive_claim_asks_the_image_lane_only_and_the_general_slot_both_lanes() {
    // Arrange
    let api = ScriptedApi::start().await;
    let mut interactive = Rig::new(&api, SlotName::Interactive, WORKER);
    let mut general = Rig::new(&api, SlotName::General, WORKER);

    // Act
    let idle_interactive = interactive.slot_loop.step().await;
    let interactive_lanes = claimed_lanes(&api.peer.requests());
    let idle_general = general.slot_loop.step().await;
    let idle_general_again = general.slot_loop.step().await;
    let general_lanes = claimed_lanes(&api.peer.requests());

    // Assert
    assert_eq!(idle_interactive, SlotStep::Idle);
    assert_eq!(idle_general, SlotStep::Idle);
    assert_eq!(idle_general_again, SlotStep::Idle);
    assert_eq!(interactive_lanes, ["image"]);
    assert_eq!(general_lanes, ["video", "image", "image", "video"]);
}
