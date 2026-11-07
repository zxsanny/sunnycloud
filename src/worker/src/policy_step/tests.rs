use media_shared::policy::{AudioPlan, AudioStream, Encoder, MediaKind, VideoStream};
use serde_json::Value;

use super::*;

fn caps() -> Capabilities {
    Capabilities {
        protocol: 1,
        vips: "8.15".to_owned(),
        ffmpeg: "7.0".to_owned(),
        libheif: "1.17".to_owned(),
        build_timestamp: 1_700_000_000,
        nvenc_probe_ok: true,
        nvenc_sessions: 2,
    }
}

fn video_probe(width: u32, height: u32, bitrate: u64, moov_first: bool) -> Probe {
    Probe {
        kind: MediaKind::Video,
        container: "mov,mp4,m4a,3gp,3g2,mj2".to_owned(),
        moov_first,
        video: Some(VideoStream {
            codec: Some("h264".to_owned()),
            profile: Some("High".to_owned()),
            width,
            height,
            rotation: None,
            bitrate: Some(bitrate),
            pix_fmt: Some("yuv420p".to_owned()),
            primaries: Some("bt709".to_owned()),
            transfer: Some("bt709".to_owned()),
            matrix: Some("bt709".to_owned()),
        }),
        audio: Some(AudioStream {
            codec: "aac".to_owned(),
        }),
        duration_ms: Some(60_000),
    }
}

fn clip(duration_ms: Option<u64>) -> Probe {
    Probe {
        duration_ms,
        ..video_probe(1920, 1080, 8_000_000, true)
    }
}

fn photo() -> Probe {
    Probe {
        kind: MediaKind::Photo,
        container: "jpeg".to_owned(),
        moov_first: false,
        video: None,
        audio: None,
        duration_ms: None,
    }
}

fn poster_at(duration_ms: u64) -> Option<u64> {
    let probe = clip(Some(duration_ms));

    match choose_step(JobKind::Poster, 1, &probe, &caps()) {
        Ok(Step::Poster(render)) => {
            assert_eq!(render.max_edge, 320);
            assert_eq!(render.output, DerivedKind::Thumb);
            render.poster_at_ms
        }
        other => panic!("expected a poster step, got {other:?}"),
    }
}

#[test]
fn poster_of_nine_and_sixty_seconds_is_taken_at_900_and_1000_ms() {
    // Arrange
    let short = 9_000;
    let long = 60_000;

    // Act
    let short_offset = poster_at(short);
    let long_offset = poster_at(long);

    // Assert
    assert_eq!(short_offset, Some(900));
    assert_eq!(long_offset, Some(1_000));
}

#[test]
fn poster_offset_follows_the_policy_at_its_boundaries() {
    // Arrange
    let durations = [0, 9_999, 10_000];

    // Act
    let offsets = durations.map(poster_at);

    // Assert
    assert_eq!(offsets, [Some(0), Some(999), Some(1_000)]);
}

#[test]
fn poster_without_a_duration_is_a_step_error() {
    // Arrange
    let probe = clip(None);

    // Act
    let step = choose_step(JobKind::Poster, 1, &probe, &caps());

    // Assert
    assert_eq!(step, Err(StepError::MissingDuration));
}

#[test]
fn stills_render_1440_then_320_from_the_params_of_the_version() {
    // Arrange
    let probe = photo();

    // Act
    let step = choose_step(JobKind::Stills, 1, &probe, &caps());

    // Assert
    let Ok(Step::Stills([first, second])) = step else {
        panic!("expected a stills step, got {step:?}");
    };
    assert_eq!(
        (first.max_edge, first.output, first.poster_at_ms),
        (1440, DerivedKind::Preview, None)
    );
    assert_eq!(
        (second.max_edge, second.output, second.poster_at_ms),
        (320, DerivedKind::Thumb, None)
    );
}

#[test]
fn unknown_params_version_is_a_step_error_for_every_kind() {
    // Arrange
    let probe = clip(Some(60_000));
    let kinds = [JobKind::Stills, JobKind::Poster, JobKind::Transcode];

    // Act
    let steps = kinds.map(|kind| choose_step(kind, 999, &probe, &caps()));

    // Assert
    for step in steps {
        assert_eq!(step, Err(StepError::UnknownParamsVersion(999)));
    }
}

#[test]
fn compliant_landscape_and_portrait_clips_are_skipped() {
    // Arrange
    let landscape = video_probe(1920, 1080, 8_000_000, true);
    let portrait = video_probe(1080, 1920, 8_000_000, true);

    // Act
    let steps = [&landscape, &portrait].map(|p| choose_step(JobKind::Transcode, 1, p, &caps()));

    // Assert
    assert_eq!(steps, [Ok(Step::SkipCompliant), Ok(Step::SkipCompliant)]);
}

#[test]
fn clip_with_the_index_at_the_end_is_remuxed() {
    // Arrange
    let probe = video_probe(1920, 1080, 8_000_000, false);

    // Act
    let step = choose_step(JobKind::Transcode, 1, &probe, &caps());

    // Assert
    assert_eq!(step, Ok(Step::Run(Plan::Remux)));
}

#[test]
fn clip_over_the_bitrate_bound_is_transcoded_on_the_cpu_even_when_nvenc_is_ready() {
    // Arrange
    let probe = video_probe(1920, 1080, 10_500_000, true);

    // Act
    let step = choose_step(JobKind::Transcode, 1, &probe, &caps());

    // Assert
    let Ok(Step::Run(plan)) = step else {
        panic!("expected a run step, got {step:?}");
    };
    let Plan::Transcode {
        encoder,
        tone_map,
        audio,
        faststart,
        ..
    } = plan
    else {
        panic!("expected a transcode plan, got {plan:?}");
    };
    assert_eq!(encoder, Encoder::Cpu);
    assert!(faststart);
    assert!(!tone_map);
    assert_eq!(audio, AudioPlan::Aac);
}

#[test]
fn transcode_plan_serialises_to_enumerations_booleans_and_numbers_only() {
    // Arrange
    let probe = video_probe(1920, 1080, 10_500_000, true);
    let Ok(Step::Run(plan)) = choose_step(JobKind::Transcode, 1, &probe, &caps()) else {
        panic!("expected a run step");
    };

    // Act
    let json = serde_json::to_value(plan).expect("a plan serialises");

    // Assert
    let fields = json
        .get("transcode")
        .and_then(Value::as_object)
        .expect("a transcode plan is one object");
    for (name, value) in fields {
        let allowed = match value {
            Value::Bool(_) | Value::Number(_) => true,
            Value::String(text) => matches!(text.as_str(), "cpu" | "nvenc" | "aac" | "none"),
            _ => false,
        };
        assert!(allowed, "field {name} holds {value}");
    }
}

#[test]
fn compliant_output_passes_the_check() {
    // Arrange
    let output = video_probe(1920, 1080, 8_000_000, true);

    // Act
    let result = check_output(&output);

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn twelve_mbit_output_is_output_invalid_permanent_without_a_cpu_rerun() {
    // Arrange
    let output = video_probe(1920, 1080, 12_000_000, true);

    // Act
    let result = check_output(&output);

    // Assert
    let Err(invalid) = result else {
        panic!("expected OutputInvalid, got {result:?}");
    };
    assert_eq!(
        invalid.failure_report(),
        FailureReport {
            code: "OutputInvalid".to_owned(),
            disposition: Disposition::Permanent,
        }
    );
    assert!(!invalid.cpu_rerun());
}

#[test]
fn remuxed_output_with_the_index_still_at_the_end_is_output_invalid() {
    // Arrange
    let output = video_probe(1920, 1080, 8_000_000, false);

    // Act
    let result = check_output(&output);

    // Assert
    assert_eq!(result, Err(OutputInvalid));
}
