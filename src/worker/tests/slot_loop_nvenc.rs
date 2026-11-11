//! AZ-3877: the NVENC preference reaches the plan sent to the decoder, and a failed GPU encode is rerun on
//! the CPU.

mod api_support;
mod slot_support;

use media_shared::decoder_protocol::{DecodeError, Request};
use media_shared::policy::{Encoder, Plan};
use media_shared::types::JobKind;
use slot_support::{
    Answer, Entry, Rig, ScriptedApi, calls, compliant_probe, loop_config, transcode_probe,
};
use sunnycloud_worker::{JobEnd, LoopConfig, SlotName};

const WORKER: &str = "worker-nvenc";

async fn transcode_rig(nvenc: bool, probe_ok: bool) -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    let config = LoopConfig {
        nvenc,
        ..loop_config()
    };
    let rig = Rig::with_config(&api, SlotName::General, WORKER, config);
    if probe_ok {
        rig.decoder.report_nvenc_ready();
    }
    rig.decoder.probes([Answer::Probe(transcode_probe())]);
    (api, rig)
}

fn encoders(rig: &Rig) -> Vec<Option<(Encoder, usize)>> {
    rig.decoder
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            Request::Transcode { plan, .. } => Some(match plan {
                Plan::Transcode { encoder, .. } => Some((encoder, seen.out_entries)),
                Plan::Remux => None,
            }),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn only_nvenc_on_with_a_passed_probe_sends_the_nvenc_plan() {
    let cases = [
        (false, false, Encoder::Cpu),
        (false, true, Encoder::Cpu),
        (true, false, Encoder::Cpu),
        (true, true, Encoder::Nvenc),
    ];
    for (nvenc, probe_ok, expected) in cases {
        // Arrange
        let (_api, mut rig) = transcode_rig(nvenc, probe_ok).await;
        rig.decoder
            .transcodes([Answer::Transcode(compliant_probe(60_000))]);

        // Act
        let end = rig.run_one().await;

        // Assert
        assert_eq!(end, JobEnd::Derived);
        assert_eq!(
            encoders(&rig),
            [Some((expected, 0))],
            "nvenc {nvenc}, probe {probe_ok}"
        );
    }
}

#[tokio::test]
async fn failed_nvenc_encode_is_rerun_once_on_the_cpu_and_the_job_succeeds() {
    // Arrange
    let (api, mut rig) = transcode_rig(true, true).await;
    rig.decoder.transcodes([
        Answer::Error(DecodeError::EncoderFailed { exit_code: 1 }),
        Answer::Transcode(compliant_probe(60_000)),
    ]);

    // Act
    let end = rig.run_one().await;

    // Assert
    assert_eq!(end, JobEnd::Derived);
    assert_eq!(
        encoders(&rig),
        [Some((Encoder::Nvenc, 0)), Some((Encoder::Cpu, 0))]
    );
    assert_eq!(
        calls(&api.peer.requests())[1..],
        ["input", "output transcode"]
    );
    assert_eq!(rig.scratch_entries(), 0);
}
