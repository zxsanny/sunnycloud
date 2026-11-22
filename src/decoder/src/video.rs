//! The video executor (E-19/c11, 10 section 4): a lossless remux or a CPU x264 transcode of
//! `in/source` into `out/result.mp4` by one ffmpeg child under the process guard, then the
//! output is probed and checked before its facts go back.
//!
//! The argv comes from [`video_args`] only. ffmpeg runs with `-progress pipe:1`, so every
//! progress block is a `Progress` event and a silence of `stall_timeout` is the guard's stall
//! kill. A clean non-zero exit of the encoder, and of a remux, is `EncoderFailed{exit_code}`:
//! the input was probed first, so the failure is the tool's, and the supervisor reruns once on
//! the CPU path (a remux as a transcode, 09 section 3).

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::MetadataExt;

use media_shared::decoder_protocol::{DecodeError, LimitKind, Response, TranscodeResult};
use media_shared::policy::{Encoder, Plan, Probe};

use crate::exec::{Exec, internal};
use crate::failure::{Failure, check_encoder};
use crate::ffprobe;
use crate::image;
use crate::params;
use crate::process::{self, Spec};
use crate::sniff::{Sniffed, read_head, sniff};
use crate::video_args::{self, Encode, RESULT};

/// The output may differ from the input by this much (10 section 4, output validation).
const DURATION_TOLERANCE_MS: u64 = 1000;

pub(crate) fn transcode(exec: &Exec<'_>, plan: Plan) -> Result<Response, Failure> {
    let input = input_facts(exec)?;
    let args = match plan {
        Plan::Remux => video_args::remux_args(exec.limits.cpu),
        Plan::Transcode {
            params_version,
            encoder,
            scale_to_short_edge,
            tone_map,
            audio,
            faststart,
        } => {
            let params = params::for_version(params_version)?;
            // The GPU path is E-19/c13 (AZ-3862, v1.1); the runner reports no NVENC, so the
            // supervisor never plans it.
            if encoder == Encoder::Nvenc {
                return Err(DecodeError::Unsupported.into());
            }
            let stream = input.video.as_ref().ok_or(DecodeError::Corrupt)?;
            let encode = Encode {
                size: video_args::target_size(
                    stream.width,
                    stream.height,
                    stream.rotation,
                    u32::from(scale_to_short_edge),
                    params.video.max_long_edge,
                ),
                tone_map,
                audio,
                faststart,
                convert: if tone_map {
                    None
                } else {
                    video_args::sdr_conversion(stream)
                },
            };
            video_args::encode_args(exec.limits.cpu, &params.video, &encode)
        }
    };
    run_ffmpeg(exec, args)?;
    validate(exec, &input).map(Response::TranscodeResult)
}

/// The facts of the input; a file that is not a video container is `Unsupported`.
fn input_facts(exec: &Exec<'_>) -> Result<Probe, Failure> {
    match image::sniffed(exec.context.input())? {
        Sniffed::Video => ffprobe::probe(exec),
        Sniffed::Still { .. } | Sniffed::Unknown => Err(DecodeError::Unsupported.into()),
    }
}

fn run_ffmpeg(exec: &Exec<'_>, args: Vec<String>) -> Result<(), Failure> {
    let out = exec.checked_out_dir()?;
    // A leftover name could be a symlink that ffmpeg would write through; removing it removes the link.
    match fs::remove_file(out.join(RESULT)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(internal("cannot clear out/result.mp4", &error));
        }
        _ => {}
    }
    let spec = Spec {
        program: &exec.tools.ffmpeg,
        args,
        env: Vec::new(),
        cwd: &out,
        work: exec.work.path(),
        input: exec.context.input(),
        limits: exec.limits,
        memory_poll: exec.config.memory_poll,
        progress: true,
    };
    let finished = process::run(&spec, exec.context)
        .map_err(|error| internal("cannot run a media tool", &error))?;
    check_encoder("ffmpeg", &finished)
}

/// The output must be a regular single-link MP4 within the size limit that probes as one H.264
/// stream of the input's duration; its probe is what goes back.
fn validate(exec: &Exec<'_>, input: &Probe) -> Result<TranscodeResult, Failure> {
    let out = exec.checked_out_dir()?;
    let file = image::open_result(&out.join(RESULT))?;
    let meta = file
        .metadata()
        .map_err(|error| internal("cannot stat the output", &error))?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() == 0 {
        return Err(DecodeError::OutputInvalid.into());
    }
    if meta.len() > exec.limits.max_output_bytes {
        return Err(Failure::limit(LimitKind::Output));
    }
    if !is_video(&file)? {
        return Err(DecodeError::OutputInvalid.into());
    }
    let output = ffprobe::probe_file(exec, &file).map_err(output_failure)?;
    verdict(input, &output)?;
    Ok(TranscodeResult {
        probe_of_output: output,
        bytes: meta.len(),
    })
}

fn is_video(file: &File) -> Result<bool, Failure> {
    let head = read_head(file).map_err(|error| internal("cannot read the output", &error))?;
    Ok(matches!(sniff(&head), Sniffed::Video))
}

/// A probe that cannot read the runner's own output says the output is bad, not the input.
fn output_failure(failure: Failure) -> Failure {
    match failure {
        Failure::Decode(DecodeError::Corrupt | DecodeError::Unsupported) => {
            DecodeError::OutputInvalid.into()
        }
        other => other,
    }
}

/// `OutputInvalid` unless the output is an MP4 whose first video stream is H.264 and whose
/// duration is within a second of the input's. An input without a duration cannot be compared;
/// an output without one is invalid.
fn verdict(input: &Probe, output: &Probe) -> Result<(), DecodeError> {
    let h264 = output
        .video
        .as_ref()
        .and_then(|video| video.codec.as_deref())
        == Some("h264");
    let mp4 = output.container.split(',').any(|name| name == "mp4");
    let duration = output.duration_ms.ok_or(DecodeError::OutputInvalid)?;
    let close = input
        .duration_ms
        .is_none_or(|expected| expected.abs_diff(duration) <= DURATION_TOLERANCE_MS);
    if h264 && mp4 && close {
        Ok(())
    } else {
        Err(DecodeError::OutputInvalid)
    }
}

#[cfg(test)]
mod tests {
    use media_shared::policy::{MediaKind, VideoStream};

    use super::*;

    fn stream(codec: Option<&str>) -> VideoStream {
        VideoStream {
            codec: codec.map(str::to_owned),
            profile: None,
            width: 1920,
            height: 1080,
            rotation: None,
            bitrate: None,
            pix_fmt: None,
            primaries: None,
            transfer: None,
            matrix: None,
        }
    }

    fn probe(codec: Option<&str>, container: &str, duration_ms: Option<u64>) -> Probe {
        Probe {
            kind: MediaKind::Video,
            container: container.to_owned(),
            moov_first: true,
            video: Some(stream(codec)),
            audio: None,
            duration_ms,
        }
    }

    fn input() -> Probe {
        probe(Some("hevc"), "mov,mp4,m4a,3gp,3g2,mj2", Some(10_000))
    }

    #[test]
    fn an_h264_mp4_of_the_input_duration_is_valid() {
        let output = probe(Some("h264"), "mov,mp4,m4a,3gp,3g2,mj2", Some(10_400));

        assert_eq!(verdict(&input(), &output), Ok(()));
    }

    #[test]
    fn the_duration_tolerance_is_one_second_either_way() {
        for (duration, valid) in [
            (9_000, true),
            (11_000, true),
            (8_999, false),
            (11_001, false),
            (8_000, false),
        ] {
            let output = probe(Some("h264"), "mov,mp4,m4a,3gp,3g2,mj2", Some(duration));

            let result = verdict(&input(), &output);

            assert_eq!(result.is_ok(), valid, "{duration}");
        }
    }

    #[test]
    fn each_failing_property_of_the_output_is_output_invalid() {
        let mp4 = "mov,mp4,m4a,3gp,3g2,mj2";
        let mut no_video = probe(Some("h264"), mp4, Some(10_000));
        no_video.video = None;
        let outputs = [
            ("no video stream", no_video),
            ("hevc", probe(Some("hevc"), mp4, Some(10_000))),
            ("codec not reported", probe(None, mp4, Some(10_000))),
            (
                "not an mp4",
                probe(Some("h264"), "matroska,webm", Some(10_000)),
            ),
            ("no duration", probe(Some("h264"), mp4, None)),
            ("two seconds short", probe(Some("h264"), mp4, Some(8_000))),
        ];

        for (name, output) in outputs {
            let result = verdict(&input(), &output);

            assert_eq!(result, Err(DecodeError::OutputInvalid), "{name}");
        }
    }

    #[test]
    fn an_input_without_a_duration_is_not_compared() {
        let output = probe(Some("h264"), "mov,mp4,m4a,3gp,3g2,mj2", Some(500));

        let result = verdict(&probe(Some("h264"), "mp4", None), &output);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_probe_failure_on_the_output_blames_the_output_and_hostile_outcomes_stay() {
        assert_eq!(
            output_failure(DecodeError::Corrupt.into()),
            Failure::Decode(DecodeError::OutputInvalid)
        );
        assert_eq!(
            output_failure(DecodeError::Unsupported.into()),
            Failure::Decode(DecodeError::OutputInvalid)
        );
        assert_eq!(output_failure(Failure::Crashed), Failure::Crashed);
        assert_eq!(
            output_failure(Failure::limit(LimitKind::Wall)),
            Failure::limit(LimitKind::Wall)
        );
    }
}
