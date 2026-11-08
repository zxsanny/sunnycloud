//! AZ-3872 AC-2, AC-3, AC-7 and AC-8: the flow of each job kind against a scripted API and a fake decoder.

mod api_support;
mod slot_support;

use media_shared::decoder_protocol::Request;
use media_shared::policy::{AudioPlan, Encoder, Plan};
use media_shared::types::JobKind;
use slot_support::{
    Answer, Entry, Rig, ScriptedApi, calls, compliant_probe, json, photo_limits, photo_probe,
    remux_probe, transcode_probe, video_limits,
};
use sunnycloud_worker::{JobEnd, SlotName};

const WORKER: &str = "worker-flow";

async fn rig() -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    let rig = Rig::new(&api, SlotName::General, WORKER);
    (api, rig)
}

async fn interactive_rig() -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    let rig = Rig::new(&api, SlotName::Interactive, WORKER);
    (api, rig)
}

fn image() -> Answer {
    Answer::Image {
        width: 1440,
        height: 1080,
    }
}

#[tokio::test]
async fn stills_job_fetches_once_renders_twice_and_posts_preview_then_thumb() {
    // Arrange
    let (api, mut rig) = interactive_rig().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills([image(), image()]);

    // Act
    let end = rig.run_one().await;

    // Assert
    let requests = api.peer.requests();
    assert_eq!(end, JobEnd::Derived);
    assert_eq!(
        calls(&requests)[1..],
        ["input", "output preview", "output thumb"]
    );
    assert_eq!(requests[2].body, b"webp edge 1440");
    assert_eq!(requests[3].body, b"webp edge 320");
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn stills_renders_use_the_two_edges_in_order_on_an_emptied_out_directory() {
    // Arrange
    let (api, mut rig) = interactive_rig().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills([image(), image()]);

    // Act
    rig.run_one().await;

    // Assert
    let seen = rig.decoder.seen();
    let renders: Vec<(u32, Option<u64>, usize)> = seen
        .iter()
        .filter_map(|seen| match &seen.request {
            Request::RenderStill {
                max_edge,
                poster_at_ms,
                ..
            } => Some((*max_edge, *poster_at_ms, seen.out_entries)),
            _ => None,
        })
        .collect();
    assert_eq!(renders, [(1440, None, 0), (320, None, 0)]);
}

#[tokio::test]
async fn poster_job_renders_the_poster_frame_and_posts_it_as_thumb() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Poster));
    rig.decoder.probes([Answer::Probe(compliant_probe(9_000))]);
    rig.decoder.stills([image()]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    assert_eq!(calls(&api.peer.requests())[1..], ["input", "output thumb"]);
    let requests = rig.decoder.requests();
    assert!(matches!(
        requests[1],
        Request::RenderStill {
            max_edge: 320,
            poster_at_ms: Some(900),
            ..
        }
    ));
}

#[tokio::test]
async fn compliant_clip_is_closed_skipped_compliant_with_no_transcode_and_no_output() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    rig.decoder.probes([Answer::Probe(compliant_probe(60_000))]);

    // Act
    let end = rig.run_one().await;

    // Assert
    let requests = api.peer.requests();
    assert_eq!(end, JobEnd::SkippedCompliant);
    assert_eq!(calls(&requests)[1..], ["input", "result"]);
    assert_eq!(json(&requests[2])["outcome"], "skipped_compliant");
    assert_eq!(rig.decoder.requests().len(), 1);
}

#[tokio::test]
async fn index_at_end_clip_is_remuxed_and_posted_as_transcode() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    rig.decoder.probes([Answer::Probe(remux_probe())]);
    rig.decoder
        .transcodes([Answer::Transcode(compliant_probe(60_000))]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Remuxed);
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output transcode"]
    );
    assert!(matches!(
        rig.decoder.requests()[1],
        Request::Transcode {
            plan: Plan::Remux,
            ..
        }
    ));
}

#[tokio::test]
async fn non_compliant_clip_is_transcoded_on_the_cpu_and_posted_as_transcode() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    rig.decoder.probes([Answer::Probe(transcode_probe())]);
    rig.decoder
        .transcodes([Answer::Transcode(compliant_probe(60_000))]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    let requests = api.peer.requests();
    assert_eq!(calls(&requests)[1..], ["input", "output transcode"]);
    assert_eq!(requests[2].header("x-duration-ms"), Some("60000"));
    assert!(matches!(
        rig.decoder.requests()[1],
        Request::Transcode {
            plan: Plan::Transcode {
                encoder: Encoder::Cpu,
                audio: AudioPlan::Aac,
                faststart: true,
                ..
            },
            ..
        }
    ));
}

#[tokio::test]
async fn already_present_preview_counts_as_done_and_the_thumb_is_posted() {
    // Arrange
    let (api, mut rig) = interactive_rig().await;
    api.enqueue(Entry::new(1, JobKind::Stills).output_status("preview", 200));
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills([image(), image()]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output preview", "output thumb"]
    );
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn photo_requests_carry_the_photo_limits_and_video_requests_the_video_limits() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    api.enqueue(Entry::new(2, JobKind::Transcode));
    rig.decoder.probes([
        Answer::Probe(photo_probe()),
        Answer::Probe(transcode_probe()),
    ]);
    rig.decoder.stills([image(), image()]);
    rig.decoder
        .transcodes([Answer::Transcode(compliant_probe(60_000))]);

    // Act: the general slot starts with the video lane, so the video job runs first.
    rig.run_one().await;
    rig.run_one().await;

    // Assert
    let limits: Vec<_> = rig
        .decoder
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            Request::RenderStill { limits, .. } | Request::Transcode { limits, .. } => Some(limits),
            _ => None,
        })
        .collect();
    assert_eq!(limits, [video_limits(), photo_limits(), photo_limits(),]);
}

#[tokio::test]
async fn jobs_are_counted_by_kind_and_result() {
    // Arrange
    let (api, mut rig) = rig().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    rig.decoder.probes([Answer::Probe(compliant_probe(60_000))]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(rig.jobs.jobs(JobKind::Transcode, &end), 1);
    assert_eq!(rig.jobs.jobs(JobKind::Stills, &end), 0);
}
