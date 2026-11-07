//! AC-1: every type of the three modules serialises to JSON and parses back equal.

mod common;

use common::*;
use media_shared::decoder_protocol::{
    Capabilities, ImageResult, Limits, SelfCheckItem, SelfCheckReport, TranscodeResult,
};
use media_shared::policy::{AudioStream, DecoderCapabilities, Encoder, EncoderPref, MediaKind};

#[test]
fn types_module_round_trips() {
    for lane in LANES {
        round_trip(&lane);
    }
    for kind in JOB_KINDS {
        round_trip(&kind);
    }
    for kind in DERIVED_KINDS {
        round_trip(&kind);
    }
    for outcome in SKIP_OUTCOMES {
        round_trip(&outcome);
    }
    for disposition in DISPOSITIONS {
        round_trip(&disposition);
    }
    for result in FAIL_RESULTS {
        round_trip(&result);
    }
    round_trip(&claimed_job());
    round_trip(&lease_extended());
    round_trip(&output_meta());
    round_trip(&failure_report());
    round_trip(&worker_status());
}

#[test]
fn types_with_absent_optionals_round_trip() {
    let meta = media_shared::types::OutputMeta {
        media_type: "image/webp".to_owned(),
        width: None,
        height: None,
        duration_ms: None,
    };

    round_trip(&meta);
}

#[test]
fn claimed_job_extremes_round_trip() {
    let mut job = claimed_job();
    job.params_version = u32::MAX;
    job.attempt = u32::MAX;
    job.lease_ms = u64::MAX;
    job.content_hash = sc_ids::FileHash::from_bytes([0xff; 32]);

    round_trip(&job);
}

#[test]
fn policy_module_round_trips() {
    round_trip(&video_probe());
    round_trip(&photo_probe());
    round_trip(&video_stream());
    round_trip(&AudioStream {
        codec: "aac".to_owned(),
    });
    round_trip(&MediaKind::Photo);
    round_trip(&MediaKind::Video);
    for encoder in [Encoder::Cpu, Encoder::Nvenc] {
        round_trip(&encoder);
        let pref: EncoderPref = encoder;
        round_trip(&pref);
    }
    for plan in plans() {
        round_trip(&plan);
    }
    for decision in decisions() {
        round_trip(&decision);
    }
    round_trip(&params());
    round_trip(&output_spec());
    let caps: DecoderCapabilities = capabilities();
    round_trip(&caps);
}

#[test]
fn decoder_protocol_module_round_trips() {
    for request in requests() {
        round_trip(&request);
    }
    for response in responses() {
        round_trip(&response);
    }
    for event in events() {
        round_trip(&event);
    }
    for error in decode_errors() {
        round_trip(&error);
    }
    for kind in LIMIT_KINDS {
        round_trip(&kind);
    }
    round_trip::<Capabilities>(&capabilities());
    round_trip::<Limits>(&limits());
    round_trip::<ImageResult>(&ImageResult {
        width: u32::MAX,
        height: 1,
        bytes: u64::MAX,
    });
    round_trip::<TranscodeResult>(&TranscodeResult {
        probe_of_output: video_probe(),
        bytes: 1,
    });
    round_trip::<SelfCheckReport>(&self_check_report());
    round_trip::<SelfCheckItem>(&SelfCheckItem {
        name: "uid".to_owned(),
        ok: true,
    });
}
