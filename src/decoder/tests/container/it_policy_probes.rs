//! IT-08-17, the real-probe part (AZ-3847 AC-3): `decide` on what `probe` of the release image
//! reports for clips the harness makes with its own ffmpeg: DS-07, the seven DS-35 edge clips and
//! the two controls (`test-data.md`), at 3 s instead of 20. Each edge clip misses one property of
//! DS-07; putting that property back in its probe makes the probe compliant, so that property
//! alone decided. The synthetic boundary, no-audio, rotation, `Params` and `Nvenc` items are in
//! `media-shared/tests/policy_rules.rs` and `job_tables.rs`.

use std::time::Duration;

use media_shared::decoder_protocol::Response;
use media_shared::policy::{AudioStream, Decision, Encoder, Plan, Probe, VideoStream, decide};

use crate::common::media::Media;
use crate::decoder::{Decoder, Flavor, release_image};
use crate::peer::probe;
use crate::refs::record;

const WAIT: Duration = Duration::from_secs(60);

/// The decision a clip's probe must get.
#[derive(Debug, Clone, Copy)]
enum Want {
    Skip,
    Remux,
    /// A transcode plan; it tone maps only an HDR clip.
    Transcode {
        tone_map: bool,
    },
}

impl Want {
    fn matches(self, decision: &Decision) -> bool {
        match (self, decision) {
            (Want::Skip, Decision::SkipCompliant) | (Want::Remux, Decision::Remux) => true,
            (
                Want::Transcode { tone_map },
                Decision::Transcode(Plan::Transcode {
                    tone_map: planned, ..
                }),
            ) => tone_map == *planned,
            _ => false,
        }
    }
}

/// What the near-miss clips other than the HDR one get.
const NEAR_MISS: Want = Want::Transcode { tone_map: false };

/// A generated clip, the decision its probe must get, and the edit that puts back in the probe
/// the DS-07 property the clip was made to miss (none for DS-07 and the controls).
struct Case {
    name: &'static str,
    clip: Vec<u8>,
    want: Want,
    repair: fn(&mut Probe),
}

fn video(probe: &mut Probe) -> &mut VideoStream {
    probe
        .video
        .as_mut()
        .expect("the probe reports a video stream")
}

fn cases(media: &Media) -> Vec<Case> {
    vec![
        Case {
            name: "DS-07 compliant",
            clip: media.compliant_mp4(),
            want: Want::Skip,
            repair: |_| {},
        },
        Case {
            name: "DS-35 10.5 Mbit/s",
            clip: media.cbr_mp4(10_500),
            want: NEAR_MISS,
            repair: |probe| video(probe).bitrate = Some(10_000_000),
        },
        Case {
            name: "DS-35 1920x1088",
            clip: media.sized_mp4(1920, 1088),
            want: NEAR_MISS,
            repair: |probe| video(probe).height = 1080,
        },
        Case {
            name: "DS-35 High 10",
            clip: media.high10_mp4(),
            want: NEAR_MISS,
            repair: |probe| video(probe).profile = Some("High".to_owned()),
        },
        Case {
            name: "DS-35 moov at end",
            clip: media.index_at_end_mp4(),
            want: Want::Remux,
            repair: |probe| probe.moov_first = true,
        },
        Case {
            name: "DS-35 HLG BT.2020 (HEVC Main 10)",
            clip: media.hlg_faststart_mp4(),
            want: Want::Transcode { tone_map: true },
            repair: |probe| {
                let stream = video(probe);
                stream.codec = Some("h264".to_owned());
                stream.profile = Some("High".to_owned());
                for tag in [
                    &mut stream.primaries,
                    &mut stream.transfer,
                    &mut stream.matrix,
                ] {
                    *tag = Some("bt709".to_owned());
                }
            },
        },
        Case {
            name: "DS-35 HEVC",
            clip: media.hevc_mp4(),
            want: NEAR_MISS,
            repair: |probe| video(probe).codec = Some("h264".to_owned()),
        },
        Case {
            name: "DS-35 MP3 audio",
            clip: media.mp3_mp4(),
            want: NEAR_MISS,
            repair: |probe| {
                probe.audio = Some(AudioStream {
                    codec: "aac".to_owned(),
                })
            },
        },
        Case {
            name: "control 9.5 Mbit/s",
            clip: media.cbr_mp4(9_500),
            want: Want::Skip,
            repair: |_| {},
        },
        Case {
            name: "control portrait 1080x1920",
            clip: media.sized_mp4(1080, 1920),
            want: Want::Skip,
            repair: |_| {},
        },
    ]
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_08_17_decide_on_real_probes_of_ds07_the_ds35_edge_clips_and_the_controls() {
    // Arrange
    let media = Media::new();
    let cases = cases(&media);
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();
    let capabilities = peer.hello();

    // Act
    let mut decided = Vec::new();
    for case in &cases {
        let job = decoder.job(&case.clip);
        let answer = peer.call(probe(&job), WAIT);
        let Response::Probe(probed) = answer else {
            panic!("{}: {answer:?}", case.name)
        };
        let decision = decide(&probed, &capabilities, Encoder::Cpu);
        let mut repaired = probed.clone();
        (case.repair)(&mut repaired);
        let repaired_decision = decide(&repaired, &capabilities, Encoder::Cpu);
        record(
            "IT-08-17",
            &format!(
                "{}: {probed:?} -> {decision:?}; repaired -> {repaired_decision:?}",
                case.name
            ),
        );
        decided.push((decision, repaired_decision));
    }

    // Assert
    for (case, (decision, repaired)) in cases.iter().zip(&decided) {
        assert!(
            case.want.matches(decision),
            "{}: decided {decision:?}, want {:?}",
            case.name,
            case.want
        );
        assert_eq!(
            *repaired,
            Decision::SkipCompliant,
            "{}: with the missed property put back",
            case.name
        );
    }
    decoder.finish();
}
