//! How a finished or failed child becomes the typed answer of a job. The mapping keys on exit
//! codes, signals and kill reasons, never on message text (10 section 3, process guard).

use media_shared::decoder_protocol::{DecodeError, LimitKind, Response};
use sc_observability::log_warn;

use crate::process::{Finished, Kill, Outcome};

const SIGKILL: i32 = 9;
const SIGXCPU: i32 = 24;
const SIGXFSZ: i32 = 25;

/// Why a job has no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    Decode(DecodeError),
    /// A child ended by a signal the limits did not raise: a crash on hostile input. Answered
    /// `Corrupt` and hostile-class.
    Crashed,
    /// The child ended but left a live process behind. Nothing it wrote can be trusted:
    /// answered `OutputInvalid` and hostile-class.
    LeftProcesses,
    /// The client cancelled or the connection ended; the loop answers `Cancelled` or nothing.
    Cancelled,
}

impl From<DecodeError> for Failure {
    fn from(error: DecodeError) -> Self {
        Failure::Decode(error)
    }
}

impl Failure {
    pub(crate) fn limit(limit: LimitKind) -> Self {
        DecodeError::LimitExceeded { limit }.into()
    }

    /// Hostile-class outcomes end the runner after the reply (10 section 3, containment); a
    /// clean `Corrupt` or `Unsupported` does not.
    pub(crate) fn is_hostile(self) -> bool {
        matches!(
            self,
            Failure::Crashed
                | Failure::LeftProcesses
                | Failure::Decode(DecodeError::LimitExceeded { .. } | DecodeError::Stalled)
        )
    }

    pub(crate) fn response(self) -> Response {
        match self {
            Failure::Decode(error) => Response::Error(error),
            Failure::Crashed => Response::Error(DecodeError::Corrupt),
            Failure::LeftProcesses => Response::Error(DecodeError::OutputInvalid),
            Failure::Cancelled => Response::Cancelled,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "{tool} ended {outcome:?} after {ran_ms} ms, last progress {silent_ms:?} ms before the end, stderr tail: {stderr_tail}"
)]
struct ChildFailed {
    tool: &'static str,
    outcome: Outcome,
    ran_ms: u128,
    silent_ms: Option<u128>,
    stderr_tail: String,
}

/// Success is a clean exit with code 0. A clean non-zero exit is the tool refusing the input
/// (`Corrupt`); the limit kills and the signals the limits raise are `LimitExceeded`; any other
/// signal is a crash on hostile input and `Corrupt` too.
pub(crate) fn check(tool: &'static str, finished: &Finished) -> Result<(), Failure> {
    check_with(tool, finished, |_| DecodeError::Corrupt)
}

/// [`check`] for the encoder (10 section 4, decided 2026-11-22): the input already probed, so a
/// clean non-zero exit is the encoder failing, `EncoderFailed{exit_code}`. It is not
/// hostile-class: the runner keeps serving, and the supervisor reruns once (09 section 3).
pub(crate) fn check_encoder(tool: &'static str, finished: &Finished) -> Result<(), Failure> {
    check_with(tool, finished, |exit_code| DecodeError::EncoderFailed {
        exit_code,
    })
}

fn check_with(
    tool: &'static str,
    finished: &Finished,
    clean_failure: fn(i32) -> DecodeError,
) -> Result<(), Failure> {
    let failure = match finished.outcome {
        Outcome::Killed(Kill::Cancelled) => return Err(Failure::Cancelled),
        Outcome::Exited(_) | Outcome::Signaled(_) if finished.stragglers => Failure::LeftProcesses,
        Outcome::Exited(0) => return Ok(()),
        Outcome::Killed(Kill::Stall) => DecodeError::Stalled.into(),
        Outcome::Killed(Kill::Wall) | Outcome::Signaled(SIGXCPU) => Failure::limit(LimitKind::Wall),
        Outcome::Killed(Kill::Memory) | Outcome::Signaled(SIGKILL) => {
            Failure::limit(LimitKind::Memory)
        }
        Outcome::Signaled(SIGXFSZ) => Failure::limit(LimitKind::Output),
        Outcome::Exited(code) => clean_failure(code).into(),
        Outcome::Signaled(_) => Failure::Crashed,
    };
    log_warn!(
        "media tool failed",
        err = &ChildFailed {
            tool,
            outcome: finished.outcome,
            ran_ms: finished
                .timing
                .decided
                .duration_since(finished.timing.started)
                .as_millis(),
            silent_ms: finished.timing.last_progress.map(|at| finished
                .timing
                .decided
                .duration_since(at)
                .as_millis()),
            stderr_tail: finished.stderr_tail.clone(),
        }
    );
    Err(failure)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::process::Timing;

    fn finished(outcome: Outcome) -> Finished {
        Finished {
            outcome,
            stdout: Vec::new(),
            stragglers: false,
            stderr_tail: String::new(),
            timing: Timing {
                started: Instant::now(),
                last_progress: None,
                decided: Instant::now(),
            },
        }
    }

    #[test]
    fn each_outcome_maps_to_its_typed_answer() {
        let cases = [
            (Outcome::Exited(1), DecodeError::Corrupt.into()),
            (Outcome::Signaled(11), Failure::Crashed),
            (
                Outcome::Signaled(SIGXFSZ),
                Failure::limit(LimitKind::Output),
            ),
            (Outcome::Signaled(SIGXCPU), Failure::limit(LimitKind::Wall)),
            (
                Outcome::Signaled(SIGKILL),
                Failure::limit(LimitKind::Memory),
            ),
            (Outcome::Killed(Kill::Wall), Failure::limit(LimitKind::Wall)),
            (
                Outcome::Killed(Kill::Memory),
                Failure::limit(LimitKind::Memory),
            ),
            (Outcome::Killed(Kill::Stall), DecodeError::Stalled.into()),
            (Outcome::Killed(Kill::Cancelled), Failure::Cancelled),
        ];

        for (outcome, expected) in cases {
            let result = check("tool", &finished(outcome));

            assert_eq!(result, Err(expected), "{outcome:?}");
        }
    }

    #[test]
    fn a_clean_non_zero_exit_of_the_encoder_is_encoder_failed_with_its_code_and_not_hostile() {
        for code in [1, 69, 255] {
            let result = check_encoder("ffmpeg", &finished(Outcome::Exited(code)));

            let expected = Failure::Decode(DecodeError::EncoderFailed { exit_code: code });
            assert_eq!(result, Err(expected));
            assert!(!expected.is_hostile());
            assert_eq!(
                expected.response(),
                Response::Error(DecodeError::EncoderFailed { exit_code: code })
            );
        }
    }

    #[test]
    fn the_encoder_check_maps_kills_signals_and_success_like_every_other_tool() {
        let cases = [
            (Outcome::Exited(0), Ok(())),
            (
                Outcome::Killed(Kill::Stall),
                Err(DecodeError::Stalled.into()),
            ),
            (
                Outcome::Killed(Kill::Wall),
                Err(Failure::limit(LimitKind::Wall)),
            ),
            (Outcome::Killed(Kill::Cancelled), Err(Failure::Cancelled)),
            (Outcome::Signaled(11), Err(Failure::Crashed)),
        ];

        for (outcome, expected) in cases {
            let result = check_encoder("ffmpeg", &finished(outcome));

            assert_eq!(result, expected, "{outcome:?}");
        }
    }

    #[test]
    fn a_clean_exit_that_left_a_live_process_behind_is_hostile_and_not_a_success() {
        let mut done = finished(Outcome::Exited(0));
        done.stragglers = true;

        let result = check("tool", &done);

        assert_eq!(result, Err(Failure::LeftProcesses));
        assert!(Failure::LeftProcesses.is_hostile());
        assert_eq!(
            Failure::LeftProcesses.response(),
            Response::Error(DecodeError::OutputInvalid)
        );
    }

    #[test]
    fn only_crashes_and_limits_are_hostile_class() {
        let hostile = [
            Failure::Crashed,
            Failure::limit(LimitKind::Pixels),
            Failure::limit(LimitKind::Wall),
            Failure::limit(LimitKind::Memory),
            Failure::limit(LimitKind::Output),
            Failure::Decode(DecodeError::Stalled),
        ];
        let clean = [
            Failure::Decode(DecodeError::Corrupt),
            Failure::Decode(DecodeError::Unsupported),
            Failure::Decode(DecodeError::OutputInvalid),
            Failure::Cancelled,
        ];

        assert!(hostile.iter().all(|failure| failure.is_hostile()));
        assert!(clean.iter().all(|failure| !failure.is_hostile()));
        assert_eq!(
            Failure::Crashed.response(),
            Response::Error(DecodeError::Corrupt)
        );
    }

    #[test]
    fn a_clean_exit_with_code_zero_is_success_whatever_stderr_says() {
        let mut done = finished(Outcome::Exited(0));
        done.stderr_tail = "Killed".into();

        assert_eq!(check("tool", &done), Ok(()));
    }
}
