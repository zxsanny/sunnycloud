//! `cpu_transcode_plan`: the plan a failed `Remux` or a failed hardware encode is rerun with.

mod common;

use common::{capabilities, video_probe};
use media_shared::policy::{AudioPlan, Decision, Encoder, Plan, cpu_transcode_plan, decide};

#[test]
fn cpu_transcode_plan_of_a_remux_probe_is_the_cpu_transcode_of_its_axes() {
    // Arrange
    let mut index_at_end = video_probe();
    index_at_end.moov_first = false;

    // Act
    let plan = cpu_transcode_plan(&index_at_end);

    // Assert
    assert_eq!(
        plan,
        Plan::Transcode {
            encoder: Encoder::Cpu,
            scale_to_short_edge: 1080,
            tone_map: false,
            audio: AudioPlan::Aac,
            faststart: true,
        }
    );
}

#[test]
fn cpu_transcode_plan_equals_the_cpu_plan_of_decide() {
    // Arrange
    let mut hevc = video_probe();
    hevc.video.as_mut().expect("video stream").codec = Some("hevc".to_owned());
    let mut hdr = video_probe();
    hdr.video.as_mut().expect("video stream").transfer = Some("smpte2084".to_owned());

    for probe in [hevc, hdr] {
        // Act
        let rerun = cpu_transcode_plan(&probe);

        // Assert
        assert_eq!(
            Decision::Transcode(rerun),
            decide(&probe, &capabilities(), Encoder::Cpu)
        );
    }
}
