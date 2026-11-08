use media_shared::decoder_protocol::{DecodeError, LimitKind, ProtocolViolation, ViolationKind};
use media_shared::types::{Disposition, FailureReport, Lane};

use super::failure::{Stop, decoder_stop, rerunnable};
use super::{lane_order, other};
use crate::api::ApiError;
use crate::decoder_client::{CallError, SlotName, Unhealthy};
use crate::policy_step::StepError;
use crate::scratch::{RejectReason, ScratchError};

fn fail(code: &str, disposition: Disposition) -> Stop {
    Stop::Fail(FailureReport {
        code: code.to_owned(),
        disposition,
    })
}

fn decode(error: DecodeError) -> CallError {
    CallError::Decode(error)
}

#[test]
fn interactive_slot_claims_the_image_lane_only_whatever_the_rotation() {
    // Arrange
    let firsts = [Lane::Image, Lane::Video];

    // Act
    let orders: Vec<_> = firsts
        .iter()
        .map(|first| lane_order(SlotName::Interactive, *first))
        .collect();

    // Assert
    assert!(orders.iter().all(|order| *order == [Lane::Image]));
}

#[test]
fn general_slot_alternates_its_first_lane_every_round() {
    // Arrange
    let mut first = Lane::Video;

    // Act
    let firsts: Vec<Lane> = (0..6)
        .map(|_| {
            let current = lane_order(SlotName::General, first)[0];
            first = other(first);
            current
        })
        .collect();

    // Assert
    assert_eq!(
        firsts,
        [
            Lane::Video,
            Lane::Image,
            Lane::Video,
            Lane::Image,
            Lane::Video,
            Lane::Image
        ]
    );
}

#[test]
fn general_slot_falls_back_to_the_other_lane_in_the_same_round() {
    // Act
    let from_video = lane_order(SlotName::General, Lane::Video);
    let from_image = lane_order(SlotName::General, Lane::Image);

    // Assert
    assert_eq!(from_video, [Lane::Video, Lane::Image]);
    assert_eq!(from_image, [Lane::Image, Lane::Video]);
}

#[test]
fn decoder_answers_end_the_job_with_the_documented_disposition() {
    // Arrange
    let cases = [
        (
            DecodeError::Unsupported,
            fail("Unsupported", Disposition::Permanent),
        ),
        (
            DecodeError::Corrupt,
            fail("Corrupt", Disposition::Permanent),
        ),
        (
            DecodeError::LimitExceeded {
                limit: LimitKind::Memory,
            },
            fail("LimitExceeded", Disposition::Permanent),
        ),
        (
            DecodeError::OutputInvalid,
            fail("OutputInvalid", Disposition::Permanent),
        ),
        (
            DecodeError::Stalled,
            fail("Stalled", Disposition::Permanent),
        ),
        (
            DecodeError::EncoderFailed { exit_code: 1 },
            fail("EncoderFailed", Disposition::Permanent),
        ),
        (
            DecodeError::ProtocolViolation,
            Stop::Unhealthy(Unhealthy::Violation),
        ),
        (DecodeError::Busy, fail("Busy", Disposition::Retry)),
    ];

    for (answer, expected) in cases {
        // Act
        let stop = decoder_stop(&decode(answer));

        // Assert
        assert_eq!(stop, expected, "{answer:?}");
    }
}

#[test]
fn a_lost_decoder_connection_is_retried() {
    // Arrange
    let closed = [
        CallError::Closed,
        CallError::Disconnected,
        CallError::Io(std::io::ErrorKind::BrokenPipe.into()),
    ];

    for error in closed {
        // Act
        let stop = decoder_stop(&error);

        // Assert
        assert_eq!(stop, fail("DecoderUnreachable", Disposition::Retry));
    }
}

#[test]
fn a_decoder_protocol_violation_is_retried() {
    // Arrange
    let error = CallError::Violation(ProtocolViolation(ViolationKind::OutOfSequence));

    // Act
    let stop = decoder_stop(&error);

    // Assert
    assert_eq!(stop, Stop::Unhealthy(Unhealthy::Violation));
}

#[test]
fn only_stalled_and_encoder_failed_are_rerun() {
    // Arrange
    let rerun = [
        decode(DecodeError::Stalled),
        decode(DecodeError::EncoderFailed { exit_code: 9 }),
    ];
    let not_rerun = [
        decode(DecodeError::Unsupported),
        decode(DecodeError::Corrupt),
        decode(DecodeError::OutputInvalid),
        decode(DecodeError::Busy),
        CallError::Closed,
    ];

    // Act and Assert
    assert!(rerun.iter().all(rerunnable));
    assert!(not_rerun.iter().all(|error| !rerunnable(error)));
}

#[test]
fn api_errors_map_to_their_reaction() {
    // Arrange
    let cases = [
        (ApiError::Forbidden, Stop::Refused),
        (ApiError::LeaseLost, Stop::LeaseLost),
        (ApiError::Gone, Stop::Obsolete),
        (ApiError::TooLarge, fail("TooLarge", Disposition::Permanent)),
        (
            ApiError::TypeRejected,
            fail("TypeRejected", Disposition::Permanent),
        ),
        (
            ApiError::HashMismatch,
            fail("HashMismatch", Disposition::Retry),
        ),
        (
            ApiError::Transient("status 503".into()),
            fail("Transient", Disposition::Retry),
        ),
        (
            ApiError::Protocol("status 400".into()),
            fail("Protocol", Disposition::Retry),
        ),
    ];

    for (error, expected) in cases {
        // Act
        let stop = Stop::from(&error);

        // Assert
        assert_eq!(stop, expected, "{error:?}");
    }
}

#[test]
fn scratch_errors_map_to_the_documented_codes() {
    // Arrange
    let too_large = ScratchError::InputTooLarge {
        declared: 2,
        free: 1,
    };
    let rejected = ScratchError::OutputRejected(RejectReason::Symlink);

    // Act
    let (large, reject) = (Stop::from(&too_large), Stop::from(&rejected));

    // Assert
    assert_eq!(large, fail("input_too_large", Disposition::Permanent));
    assert_eq!(reject, fail("output_rejected", Disposition::Permanent));
}

#[test]
fn step_errors_map_to_a_disposition() {
    // Act
    let version = Stop::from(StepError::UnknownParamsVersion(7));
    let duration = Stop::from(StepError::MissingDuration);

    // Assert
    assert_eq!(version, fail("UnknownParamsVersion", Disposition::Retry));
    assert_eq!(duration, fail("MissingDuration", Disposition::Permanent));
}

#[test]
fn every_unhealthy_cause_fails_the_job_with_retry() {
    // Arrange
    let causes = [
        (Unhealthy::Silent, "DecoderSilent"),
        (Unhealthy::WallLimit, "WallLimit"),
        (Unhealthy::Violation, "ProtocolViolation"),
    ];

    for (cause, code) in causes {
        // Act
        let report = cause.report();

        // Assert
        assert_eq!(report.code, code);
        assert_eq!(report.disposition, Disposition::Retry);
    }
}
