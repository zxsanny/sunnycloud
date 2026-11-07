//! AC-1 to AC-3: the job-kind table, the `Params` snapshot per `params_version`, the accepted
//! output types and the `JobSpec` the planner returns.

mod common;

use common::*;
use media_shared::policy::{
    AudioCodec, CURRENT_PARAMS_VERSION, H264Profile, JobSpec, OutputSpec, Params, StillFormat,
    VideoCodec,
};
use media_shared::types::{DerivedKind, JobKind, Lane};

const WEBP_OR_JPEG: [&str; 2] = ["image/webp", "image/jpeg"];

#[test]
fn stills_run_in_the_image_lane_and_produce_thumb_and_preview() {
    assert_eq!(JobKind::Stills.lane(), Lane::Image);
    assert_eq!(
        JobKind::Stills.outputs(),
        [DerivedKind::Thumb, DerivedKind::Preview]
    );
}

#[test]
fn poster_runs_in_the_video_lane_and_produces_thumb() {
    assert_eq!(JobKind::Poster.lane(), Lane::Video);
    assert_eq!(JobKind::Poster.outputs(), [DerivedKind::Thumb]);
}

#[test]
fn transcode_runs_in_the_video_lane_and_produces_transcode() {
    assert_eq!(JobKind::Transcode.lane(), Lane::Video);
    assert_eq!(JobKind::Transcode.outputs(), [DerivedKind::Transcode]);
}

#[test]
fn the_lane_of_every_kind_is_the_generated_lane_column() {
    for kind in JOB_KINDS {
        let column = match kind {
            JobKind::Stills => "image",
            JobKind::Poster | JobKind::Transcode => "video",
        };

        assert_eq!(json_of(&kind.lane()), format!("\"{column}\""), "{kind:?}");
    }
}

#[test]
fn every_kind_has_outputs_and_no_output_is_listed_twice() {
    for kind in JOB_KINDS {
        let outputs = kind.outputs();

        assert!(!outputs.is_empty(), "{kind:?}");
        for (index, output) in outputs.iter().enumerate() {
            assert!(!outputs[index + 1..].contains(output), "{kind:?}");
        }
    }
}

#[test]
fn params_version_1_is_the_frozen_snapshot() {
    let params = Params::for_version(1).expect("version 1 exists");

    assert_eq!(
        json_of(params),
        concat!(
            r#"{"thumb_edge":320,"preview_edge":1440,"still_format":"webp","apply_orientation":true,"#,
            r#""video":{"codec":"h264","profile":"high","max_short_side":1080,"max_long_side":1920,"#,
            r#""audio":"aac","faststart":true,"tone_map_to_sdr":true}}"#
        )
    );
}

#[test]
fn params_version_1_values_are_the_ones_the_contract_lists() {
    let params = Params::for_version(1).expect("version 1 exists");

    assert_eq!(params.thumb_edge, 320);
    assert_eq!(params.preview_edge, 1440);
    assert_eq!(params.still_format, StillFormat::Webp);
    assert!(params.apply_orientation);
    assert_eq!(params.video.codec, VideoCodec::H264);
    assert_eq!(params.video.profile, H264Profile::High);
    assert_eq!(params.video.max_short_side, 1080);
    assert_eq!(params.video.max_long_side, 1920);
    assert_eq!(params.video.audio, AudioCodec::Aac);
    assert!(params.video.faststart);
    assert!(params.video.tone_map_to_sdr);
}

#[test]
fn the_current_version_is_1_and_unknown_versions_have_no_params() {
    assert_eq!(CURRENT_PARAMS_VERSION, 1);
    assert!(Params::for_version(CURRENT_PARAMS_VERSION).is_some());
    assert!(Params::for_version(0).is_none());
    assert!(Params::for_version(2).is_none());
    assert!(Params::for_version(u32::MAX).is_none());
}

#[test]
fn a_params_value_with_an_unknown_codec_is_rejected() {
    let json = json_of(&params()).replace("\"h264\"", "\"h265\"");

    assert!(serde_json::from_str::<Params>(&json).is_err());
}

#[test]
fn thumb_and_preview_accept_webp_and_jpeg_only() {
    for kind in [DerivedKind::Thumb, DerivedKind::Preview] {
        let spec = OutputSpec::for_kind(kind, &params());

        assert_eq!(spec.media_types, WEBP_OR_JPEG, "{kind:?}");
        for accepted in WEBP_OR_JPEG {
            assert!(spec.accepts(accepted), "{kind:?} {accepted}");
        }
        for rejected in [
            "image/png",
            "image/gif",
            "image/heic",
            "image/svg+xml",
            "video/mp4",
            "application/octet-stream",
        ] {
            assert!(!spec.accepts(rejected), "{kind:?} {rejected}");
        }
    }
}

#[test]
fn transcode_accepts_mp4_only() {
    let spec = OutputSpec::for_kind(DerivedKind::Transcode, &params());

    assert!(spec.accepts("video/mp4"));
    for rejected in [
        "video/quicktime",
        "video/webm",
        "video/x-matroska",
        "image/webp",
    ] {
        assert!(!spec.accepts(rejected), "{rejected}");
    }
}

#[test]
fn a_media_type_matches_exactly_without_case_parameters_or_spaces() {
    let spec = OutputSpec::for_kind(DerivedKind::Thumb, &params());

    for rejected in [
        "IMAGE/WEBP",
        "image/webp; charset=binary",
        " image/webp",
        "image/webp ",
        "image/",
        "",
    ] {
        assert!(!spec.accepts(rejected), "{rejected:?}");
    }
}

#[test]
fn the_still_edges_come_from_params_and_a_transcode_has_none() {
    let params = params();

    assert_eq!(
        OutputSpec::for_kind(DerivedKind::Thumb, &params).max_edge,
        Some(320)
    );
    assert_eq!(
        OutputSpec::for_kind(DerivedKind::Preview, &params).max_edge,
        Some(1440)
    );
    assert_eq!(
        OutputSpec::for_kind(DerivedKind::Transcode, &params).max_edge,
        None
    );
}

#[test]
fn a_job_spec_carries_what_the_enqueue_needs() {
    let claimed = claimed_job();

    let spec = JobSpec {
        account_id: uuid::Uuid::from_u128(1),
        revision_id: uuid::Uuid::from_u128(2),
        kind: claimed.kind,
        content_hash: claimed.content_hash,
        params_version: CURRENT_PARAMS_VERSION,
    };

    assert_eq!(spec.kind.lane(), Lane::Video);
    assert_eq!(spec.content_hash, claimed.content_hash);
    assert!(Params::for_version(spec.params_version).is_some());
}
