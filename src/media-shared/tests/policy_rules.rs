//! Table tests of the media policy (AZ-3838) over synthetic probes.

use media_shared::decoder_protocol::Capabilities;
use media_shared::policy::{
    AudioPlan, AudioStream, Decision, Encoder, MediaKind, Plan, Probe, VideoStream, decide,
    is_compliant, poster_offset_ms, supported_kinds_for,
};
use media_shared::types::JobKind;

fn stream() -> VideoStream {
    VideoStream {
        codec: Some("h264".to_owned()),
        profile: Some("High".to_owned()),
        width: 1920,
        height: 1080,
        rotation: None,
        bitrate: Some(8_000_000),
        pix_fmt: Some("yuv420p".to_owned()),
        primaries: Some("bt709".to_owned()),
        transfer: Some("bt709".to_owned()),
        matrix: Some("bt709".to_owned()),
    }
}

fn probe_with(edit: impl FnOnce(&mut VideoStream)) -> Probe {
    let mut video = stream();
    edit(&mut video);
    Probe {
        kind: MediaKind::Video,
        container: "mov,mp4,m4a,3gp,3g2,mj2".to_owned(),
        moov_first: true,
        video: Some(video),
        audio: Some(AudioStream {
            codec: "aac".to_owned(),
        }),
        duration_ms: Some(30_000),
    }
}

fn compliant() -> Probe {
    probe_with(|_| {})
}

fn capabilities() -> Capabilities {
    Capabilities {
        protocol: 1,
        vips: "8.16.0".to_owned(),
        ffmpeg: "8.0.1".to_owned(),
        libheif: "1.22.0".to_owned(),
        build_timestamp: 1_790_000_000,
        nvenc_probe_ok: false,
        nvenc_sessions: 0,
    }
}

fn hdr_probe() -> Probe {
    probe_with(|v| {
        v.primaries = Some("bt2020".to_owned());
        v.transfer = Some("arib-std-b67".to_owned());
        v.matrix = Some("bt2020nc".to_owned());
    })
}

/// Probes that fail exactly one axis, with the axis name.
fn non_compliant() -> Vec<(&'static str, Probe)> {
    let mut mp3 = compliant();
    mp3.audio = Some(AudioStream {
        codec: "mp3".to_owned(),
    });
    let mut mkv = compliant();
    mkv.container = "matroska,webm".to_owned();
    vec![
        (
            "bitrate 10.5 Mbit/s",
            probe_with(|v| v.bitrate = Some(10_500_000)),
        ),
        (
            "bitrate over by one",
            probe_with(|v| v.bitrate = Some(10_000_001)),
        ),
        ("height 1088", probe_with(|v| v.height = 1088)),
        ("width 1921", probe_with(|v| v.width = 1921)),
        (
            "profile High 10",
            probe_with(|v| v.profile = Some("High 10".to_owned())),
        ),
        (
            "codec hevc",
            probe_with(|v| v.codec = Some("hevc".to_owned())),
        ),
        ("audio mp3", mp3),
        ("HLG BT.2020", hdr_probe()),
        ("container matroska", mkv),
    ]
}

fn transcode_plan(decision: Decision) -> Plan {
    match decision {
        Decision::Transcode(plan) => plan,
        other => panic!("expected a transcode, got {other:?}"),
    }
}

#[test]
fn compliant_boundaries_hold() {
    // Arrange
    let mut no_audio = compliant();
    no_audio.audio = None;
    let cases = [
        (
            "10,000,000 bit/s inclusive",
            probe_with(|v| v.bitrate = Some(10_000_000)),
        ),
        ("1920x1080", probe_with(|_| {})),
        (
            "1080x1920",
            probe_with(|v| (v.width, v.height) = (1080, 1920)),
        ),
        (
            "Baseline",
            probe_with(|v| v.profile = Some("Baseline".to_owned())),
        ),
        ("Main", probe_with(|v| v.profile = Some("Main".to_owned()))),
        ("High", probe_with(|v| v.profile = Some("High".to_owned()))),
        ("no audio", no_audio),
    ];

    for (name, probe) in cases {
        // Act
        let compliant = is_compliant(&probe);

        // Assert
        assert!(compliant, "{name} must be compliant");
    }
}

#[test]
fn one_failed_axis_is_not_compliant() {
    for (name, probe) in non_compliant() {
        // Act
        let compliant = is_compliant(&probe);

        // Assert
        assert!(!compliant, "{name} must not be compliant");
    }
}

#[test]
fn rotation_never_changes_compliance() {
    // Arrange
    let sizes = [
        (1920, 1080, true),
        (1080, 1920, true),
        (1920, 1088, false),
        (1088, 1920, false),
    ];

    for (width, height, expected) in sizes {
        for rotation in [
            None,
            Some(0),
            Some(90),
            Some(-90),
            Some(180),
            Some(270),
            Some(360),
        ] {
            let probe = probe_with(|v| {
                v.width = width;
                v.height = height;
                v.rotation = rotation;
            });

            // Act
            let compliant = is_compliant(&probe);

            // Assert
            assert_eq!(
                compliant, expected,
                "{width}x{height} rotation {rotation:?}"
            );
        }
    }
}

#[test]
fn rotation_swaps_no_sides_for_a_tall_clip() {
    // Arrange: the long side is the height, so a 90 degree rotation must not make 1080 the long side.
    let probe = probe_with(|v| {
        v.width = 1080;
        v.height = 1920;
        v.rotation = Some(90);
    });

    // Act
    let compliant = is_compliant(&probe);

    // Assert
    assert!(compliant);
}

#[test]
fn unreported_axis_is_not_compliant() {
    // Arrange
    let cases = [
        ("codec", probe_with(|v| v.codec = None)),
        ("profile", probe_with(|v| v.profile = None)),
        ("bitrate", probe_with(|v| v.bitrate = None)),
        ("primaries", probe_with(|v| v.primaries = None)),
        ("transfer", probe_with(|v| v.transfer = None)),
        ("matrix", probe_with(|v| v.matrix = None)),
    ];

    for (name, probe) in cases {
        // Act
        let compliant = is_compliant(&probe);

        // Assert
        assert!(!compliant, "unreported {name} must not be compliant");
    }
}

#[test]
fn index_at_end_is_not_compliant() {
    // Arrange
    let mut probe = compliant();
    probe.moov_first = false;

    // Act
    let compliant = is_compliant(&probe);

    // Assert
    assert!(!compliant);
}

#[test]
fn photo_and_streamless_probes_are_not_compliant() {
    // Arrange
    let mut photo = compliant();
    photo.kind = MediaKind::Photo;
    let mut streamless = compliant();
    streamless.video = None;

    // Act
    let results = [is_compliant(&photo), is_compliant(&streamless)];

    // Assert
    assert_eq!(results, [false, false]);
}

#[test]
fn plain_mp4_container_name_is_accepted() {
    // Arrange
    let mut probe = compliant();
    probe.container = "mp4".to_owned();

    // Act
    let compliant = is_compliant(&probe);

    // Assert
    assert!(compliant);
}

#[test]
fn decide_skips_a_compliant_clip() {
    // Arrange
    let probe = compliant();

    // Act
    let decision = decide(&probe, &capabilities(), Encoder::Cpu);

    // Assert
    assert_eq!(decision, Decision::SkipCompliant);
}

#[test]
fn decide_remuxes_only_when_the_index_is_the_sole_failure() {
    // Arrange
    let mut index_at_end = compliant();
    index_at_end.moov_first = false;
    let mut index_at_end_and_hevc = probe_with(|v| v.codec = Some("hevc".to_owned()));
    index_at_end_and_hevc.moov_first = false;

    // Act
    let remux = decide(&index_at_end, &capabilities(), Encoder::Cpu);
    let transcode = decide(&index_at_end_and_hevc, &capabilities(), Encoder::Cpu);

    // Assert
    assert_eq!(remux, Decision::Remux);
    assert!(matches!(transcode, Decision::Transcode(_)));
}

#[test]
fn decide_transcodes_every_failed_axis_with_the_fixed_plan() {
    for (name, probe) in non_compliant() {
        // Act
        let plan = transcode_plan(decide(&probe, &capabilities(), Encoder::Cpu));

        // Assert
        let expected_tone_map = name == "HLG BT.2020";
        assert_eq!(
            plan,
            Plan::Transcode {
                encoder: Encoder::Cpu,
                scale_to_short_edge: 1080,
                tone_map: expected_tone_map,
                audio: AudioPlan::Aac,
                faststart: true,
            },
            "{name}"
        );
    }
}

#[test]
fn decide_transcodes_a_non_mp4_container_without_remux() {
    // Arrange
    let mut mkv = compliant();
    mkv.container = "matroska,webm".to_owned();
    mkv.moov_first = false;

    // Act
    let decision = decide(&mkv, &capabilities(), Encoder::Cpu);

    // Assert
    assert!(matches!(decision, Decision::Transcode(_)));
}

#[test]
fn decide_plans_no_audio_for_a_silent_source() {
    // Arrange
    let mut probe = probe_with(|v| v.codec = Some("hevc".to_owned()));
    probe.audio = None;

    // Act
    let plan = transcode_plan(decide(&probe, &capabilities(), Encoder::Cpu));

    // Assert
    assert!(matches!(
        plan,
        Plan::Transcode {
            audio: AudioPlan::None,
            ..
        }
    ));
}

#[test]
fn decide_tone_maps_pq_as_well_as_hlg() {
    // Arrange
    let probe = probe_with(|v| v.transfer = Some("smpte2084".to_owned()));

    // Act
    let plan = transcode_plan(decide(&probe, &capabilities(), Encoder::Cpu));

    // Assert
    assert!(matches!(plan, Plan::Transcode { tone_map: true, .. }));
}

#[test]
fn encoder_preference_changes_only_the_encoder() {
    for (name, probe) in non_compliant() {
        // Arrange
        let caps = capabilities();

        // Act
        let cpu = transcode_plan(decide(&probe, &caps, Encoder::Cpu));
        let nvenc = transcode_plan(decide(&probe, &caps, Encoder::Nvenc));

        // Assert
        let (
            Plan::Transcode {
                encoder: ce,
                scale_to_short_edge: cs,
                tone_map: ct,
                audio: ca,
                faststart: cf,
            },
            Plan::Transcode {
                encoder: ne,
                scale_to_short_edge: ns,
                tone_map: nt,
                audio: na,
                faststart: nf,
            },
        ) = (cpu, nvenc)
        else {
            panic!("{name}: expected two transcode plans");
        };
        assert_eq!((ce, ne), (Encoder::Cpu, Encoder::Nvenc), "{name}");
        assert_eq!((cs, ct, ca, cf), (ns, nt, na, nf), "{name}");
    }
}

#[test]
fn encoder_preference_does_not_touch_skip_or_remux() {
    // Arrange
    let mut index_at_end = compliant();
    index_at_end.moov_first = false;

    // Act
    let skip = decide(&compliant(), &capabilities(), Encoder::Nvenc);
    let remux = decide(&index_at_end, &capabilities(), Encoder::Nvenc);

    // Assert
    assert_eq!((skip, remux), (Decision::SkipCompliant, Decision::Remux));
}

#[test]
fn poster_offset_table() {
    // Arrange
    let cases = [
        (0, 0),
        (500, 50),
        (5_000, 500),
        (9_999, 999),
        (10_000, 1_000),
        (10_001, 1_000),
        (600_000, 1_000),
        (u64::MAX, 1_000),
    ];

    for (duration, expected) in cases {
        // Act
        let offset = poster_offset_ms(duration);

        // Assert
        assert_eq!(offset, expected, "duration {duration}");
        assert!(
            offset <= duration,
            "offset {offset} beyond duration {duration}"
        );
    }
}

#[test]
fn supported_kinds_table() {
    // Arrange
    let cases: [(&str, Vec<JobKind>); 10] = [
        ("image/jpeg", vec![JobKind::Stills]),
        ("image/png", vec![JobKind::Stills]),
        ("image/webp", vec![JobKind::Stills]),
        ("image/gif", vec![JobKind::Stills]),
        ("image/heic", vec![JobKind::Stills]),
        ("video/mp4", vec![JobKind::Poster, JobKind::Transcode]),
        ("video/quicktime", vec![JobKind::Poster, JobKind::Transcode]),
        (
            "video/x-matroska",
            vec![JobKind::Poster, JobKind::Transcode],
        ),
        ("image/svg+xml", vec![]),
        ("application/octet-stream", vec![]),
    ];

    for (media_type, expected) in cases {
        // Act
        let kinds = supported_kinds_for(media_type);

        // Assert
        assert_eq!(kinds, expected, "{media_type}");
    }
}

#[test]
fn unlisted_and_malformed_media_types_get_no_jobs() {
    for media_type in [
        "",
        "video/webm",
        "image/avif",
        "IMAGE/JPEG",
        "image/jpeg; charset=x",
        " video/mp4",
    ] {
        // Act
        let kinds = supported_kinds_for(media_type);

        // Assert
        assert!(kinds.is_empty(), "{media_type:?}");
    }
}
