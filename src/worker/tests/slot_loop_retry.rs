//! AZ-3872 AC-4 and AC-5: the rerun inside the claim and the disposition of every failure.

mod api_support;
mod slot_support;

use media_shared::decoder_protocol::{DecodeError, LimitKind, Request};
use media_shared::policy::{Encoder, Plan};
use media_shared::types::{Disposition, FailureReport, JobKind};
use slot_support::{
    Answer, Entry, Rig, ScriptedApi, calls, compliant_probe, json, loop_config, photo_probe,
    remux_probe, transcode_probe,
};
use sunnycloud_worker::{JobEnd, KindConfig, SlotName};

const WORKER: &str = "worker-retry";

fn failed(code: &str, disposition: Disposition) -> JobEnd {
    JobEnd::Failed(FailureReport {
        code: code.to_owned(),
        disposition,
    })
}

async fn transcode_rig() -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    let rig = Rig::new(&api, SlotName::General, WORKER);
    rig.decoder.probes([Answer::Probe(transcode_probe())]);
    (api, rig)
}

async fn photo_rig() -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    let rig = Rig::new(&api, SlotName::Interactive, WORKER);
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    (api, rig)
}

fn transcodes(rig: &Rig) -> Vec<(Plan, usize)> {
    rig.decoder
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            Request::Transcode { plan, .. } => Some((plan, seen.out_entries)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn stalled_encode_is_rerun_once_on_a_cleared_out_directory_with_the_cpu_encoder() {
    // Arrange
    let (api, mut rig) = transcode_rig().await;
    rig.decoder.transcodes([
        Answer::Error(DecodeError::Stalled),
        Answer::Transcode(compliant_probe(60_000)),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    let runs = transcodes(&rig);
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|(plan, out)| {
        *out == 0
            && matches!(
                plan,
                Plan::Transcode {
                    encoder: Encoder::Cpu,
                    ..
                }
            )
    }));
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output transcode"]
    );
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn second_stall_fails_the_job_permanent() {
    // Arrange
    let (api, mut rig) = transcode_rig().await;
    rig.decoder.transcodes([
        Answer::Error(DecodeError::Stalled),
        Answer::Error(DecodeError::Stalled),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("Stalled", Disposition::Permanent));
    let requests = api.peer.requests();
    assert_eq!(calls(&requests)[1..], ["input", "failure"]);
    assert_eq!(json(&requests[2])["code"], "Stalled");
    assert_eq!(json(&requests[2])["disposition"], "permanent");
    assert_eq!(transcodes(&rig).len(), 2);
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn failed_encoder_is_rerun_once_with_the_cpu_encoder() {
    // Arrange
    let (_api, mut rig) = transcode_rig().await;
    rig.decoder.transcodes([
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
        Answer::Transcode(compliant_probe(60_000)),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    let runs = transcodes(&rig);
    assert_eq!(runs.len(), 2);
    assert!(matches!(
        runs[1].0,
        Plan::Transcode {
            encoder: Encoder::Cpu,
            ..
        }
    ));
}

#[tokio::test]
async fn second_encoder_failure_fails_the_job_permanent() {
    // Arrange
    let (_api, mut rig) = transcode_rig().await;
    rig.decoder.transcodes([
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("EncoderFailed", Disposition::Permanent));
    assert_eq!(transcodes(&rig).len(), 2);
}

#[tokio::test]
async fn failed_remux_is_rerun_once_as_a_cpu_transcode_on_a_cleared_out_directory() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    let mut rig = Rig::new(&api, SlotName::General, WORKER);
    rig.decoder.probes([Answer::Probe(remux_probe())]);
    rig.decoder.transcodes([
        Answer::Error(DecodeError::Stalled),
        Answer::Transcode(compliant_probe(60_000)),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    let runs = transcodes(&rig);
    assert_eq!(runs[0], (Plan::Remux, 0));
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].1, 0);
    assert!(matches!(
        runs[1].0,
        Plan::Transcode {
            encoder: Encoder::Cpu,
            ..
        }
    ));
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output transcode"]
    );
}

#[tokio::test]
async fn second_failure_of_a_remux_rerun_fails_permanent() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    let mut rig = Rig::new(&api, SlotName::General, WORKER);
    rig.decoder.probes([Answer::Probe(remux_probe())]);
    rig.decoder.transcodes([
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("EncoderFailed", Disposition::Permanent));
    assert_eq!(transcodes(&rig).len(), 2);
}

#[tokio::test]
async fn stalled_render_is_rerun_once_and_the_rerun_budget_is_per_claim() {
    // Arrange
    let (api, mut rig) = photo_rig().await;
    let image = || Answer::Image {
        width: 1440,
        height: 1080,
    };
    rig.decoder.stills([
        Answer::Error(DecodeError::Stalled),
        image(),
        Answer::Error(DecodeError::Stalled),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("Stalled", Disposition::Permanent));
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output preview", "failure"]
    );
}

#[tokio::test]
async fn non_compliant_output_fails_permanent_without_a_rerun_or_a_post() {
    // Arrange
    let (api, mut rig) = transcode_rig().await;
    rig.decoder
        .transcodes([Answer::Transcode(transcode_probe())]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("OutputInvalid", Disposition::Permanent));
    assert_eq!(calls(&api.peer.requests())[1..], ["input", "failure"]);
    assert_eq!(transcodes(&rig).len(), 1);
}

#[tokio::test]
async fn permanent_decoder_answers_fail_at_the_first_attempt() {
    let cases = [
        (DecodeError::Unsupported, "Unsupported"),
        (DecodeError::Corrupt, "Corrupt"),
        (
            DecodeError::LimitExceeded {
                limit: LimitKind::Pixels,
            },
            "LimitExceeded",
        ),
        (DecodeError::OutputInvalid, "OutputInvalid"),
    ];
    for (answer, code) in cases {
        // Arrange
        let (api, mut rig) = photo_rig().await;
        rig.decoder.stills_fall_back_to(Answer::Error(answer));

        // Act
        let end = rig.run_one().await;

        // Assert
        assert_eq!(end, failed(code, Disposition::Permanent), "{answer:?}");
        let requests = api.peer.requests();
        assert_eq!(calls(&requests)[1..], ["input", "failure"], "{answer:?}");
        assert_eq!(json(&requests[2])["disposition"], "permanent");
        assert_eq!(rig.decoder.requests().len(), 2, "{answer:?}: one render");
        assert_eq!(rig.scratch_entries(), 0);
    }
}

#[tokio::test]
async fn socket_closed_mid_job_fails_retry_and_the_slot_reconnects_for_the_next_job() {
    // Arrange
    let (api, mut rig) = photo_rig().await;
    api.enqueue(Entry::new(2, JobKind::Stills));
    let image = || Answer::Image {
        width: 1440,
        height: 1080,
    };
    rig.decoder.stills([Answer::Hang, image(), image()]);
    rig.decoder
        .probes([Answer::Probe(photo_probe()), Answer::Probe(photo_probe())]);

    // Act
    let first = rig.run_one().await;
    let second = rig.run_one().await;

    // Assert
    assert_eq!(first, failed("DecoderUnreachable", Disposition::Retry));
    assert_eq!(second, JobEnd::Derived);
    assert_eq!(rig.decoder.hellos(), 2);
    assert_eq!(rig.decoder_metrics.restarts(SlotName::Interactive), 1);
    let requests = api.peer.requests();
    assert_eq!(json(&requests[2])["disposition"], "retry");
}

#[tokio::test]
async fn fetch_server_error_fails_retry_and_the_decoder_sees_no_job() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills).input_status(503));
    let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("Transient", Disposition::Retry));
    assert_eq!(calls(&api.peer.requests())[1..], ["input", "failure"]);
    assert!(rig.decoder.requests().is_empty());
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn hash_mismatch_fails_retry_and_no_byte_reaches_the_decoder() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills).with_wrong_hash());
    let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("HashMismatch", Disposition::Retry));
    assert!(rig.decoder.requests().is_empty());
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn input_that_cannot_fit_the_scratch_mount_fails_permanent_before_any_fetch() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    let mut config = loop_config();
    config.photo = KindConfig {
        max_input: 1 << 62,
        ..config.photo
    };
    let mut rig = Rig::with_config(&api, SlotName::Interactive, WORKER, config);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, failed("input_too_large", Disposition::Permanent));
    assert_eq!(calls(&api.peer.requests())[1..], ["failure"]);
    assert!(rig.decoder.requests().is_empty());
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn output_the_decoder_never_wrote_fails_permanent_as_output_rejected() {
    for answer in [Answer::ImageWithoutFile, Answer::ImageAsSymlink] {
        // Arrange
        let (api, mut rig) = photo_rig().await;
        rig.decoder.stills_fall_back_to(answer.clone());

        // Act
        let end = rig.run_one().await;

        // Assert
        assert_eq!(
            end,
            failed("output_rejected", Disposition::Permanent),
            "{answer:?}"
        );
        assert_eq!(calls(&api.peer.requests())[1..], ["input", "failure"]);
        assert_eq!(rig.scratch_entries(), 0);
    }
}

#[tokio::test]
async fn the_api_rejecting_an_output_fails_permanent_and_an_api_error_fails_retry() {
    let cases = [
        (413, "TooLarge", Disposition::Permanent),
        (422, "TypeRejected", Disposition::Permanent),
        (503, "Transient", Disposition::Retry),
    ];
    for (status, code, disposition) in cases {
        // Arrange
        let api = ScriptedApi::start().await;
        api.enqueue(Entry::new(1, JobKind::Stills).output_status("preview", status));
        let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);
        rig.decoder.probes([Answer::Probe(photo_probe())]);
        rig.decoder.stills_fall_back_to(Answer::Image {
            width: 1440,
            height: 1080,
        });

        // Act
        let end = rig.run_one().await;

        // Assert
        assert_eq!(end, failed(code, disposition), "status {status}");
        assert_eq!(
            calls(&api.peer.requests())[1..],
            ["input", "output preview", "failure"]
        );
    }
}

#[tokio::test]
async fn unknown_params_version_fails_retry_and_a_poster_without_duration_fails_permanent() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Poster).params_version(99));
    api.enqueue(Entry::new(2, JobKind::Poster));
    let mut rig = Rig::new(&api, SlotName::General, WORKER);
    let mut no_duration = compliant_probe(0);
    no_duration.duration_ms = None;
    rig.decoder.probes([
        Answer::Probe(compliant_probe(9_000)),
        Answer::Probe(no_duration),
    ]);

    // Act
    let version = rig.run_one().await;
    let duration = rig.run_one().await;

    // Assert
    assert_eq!(version, failed("UnknownParamsVersion", Disposition::Retry));
    assert_eq!(duration, failed("MissingDuration", Disposition::Permanent));
}
