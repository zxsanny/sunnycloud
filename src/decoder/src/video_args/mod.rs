//! The ffmpeg argument vectors of the video executor (10 section 4) and the output size rule.
//! Pure functions: every argument comes from the `Plan` enumeration, the `Params` table and the
//! probe of the input, never from request text. The input is always `/proc/self/fd/9` and the
//! output name is relative to the child's working directory `out/`.

use media_shared::policy::AudioPlan;

use crate::ffprobe::input_options;
use crate::params::VideoParams;
use crate::process::input_path;

mod colour;
mod size;

pub(crate) use colour::sdr_conversion;
pub(crate) use size::target_size;

pub(crate) const RESULT: &str = "result.mp4";

/// The tone-mapping chain fixed by gate S5 (10 section 13.2). The downscale must come before it.
pub(crate) const HDR_TO_SDR: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

/// What a CPU transcode does beyond the `Params` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Encode {
    /// The output width and height, as [`target_size`] returns them.
    pub size: (u32, u32),
    pub tone_map: bool,
    pub audio: AudioPlan,
    pub faststart: bool,
    /// The conversion of an SDR input that is not BT.709 ([`sdr_conversion`]); unused with `tone_map`.
    pub convert: Option<String>,
}

fn owned<const N: usize>(items: [&str; N]) -> Vec<String> {
    items.into_iter().map(str::to_owned).collect()
}

/// Options shared by the remux and the encode: the S5 whitelists, the thread caps, the fixed
/// input, the first video and first audio stream only, and nothing else carried over.
fn head(cpu: u32, audio: AudioPlan) -> Vec<String> {
    let cpu = cpu.to_string();
    let mut args = owned([
        "-nostdin",
        "-hide_banner",
        "-nostats",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
    ]);
    args.extend(input_options());
    args.extend(owned(["-threads", &cpu, "-filter_threads", &cpu, "-i"]));
    args.push(input_path());
    args.extend(owned(["-map", "0:v:0"]));
    match audio {
        // Optional: a plan made from a probe that saw audio must not fail on a clip without.
        AudioPlan::Aac => args.extend(owned(["-map", "0:a:0?"])),
        AudioPlan::None => args.push("-an".to_owned()),
    }
    args.extend(owned([
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
    ]));
    args
}

fn tail(args: &mut Vec<String>) {
    args.extend(owned(["-f", "mp4", "-y", RESULT]));
}

/// `Plan::Remux`: the streams are copied, the index goes to the front, no filter.
pub(crate) fn remux_args(cpu: u32) -> Vec<String> {
    let mut args = head(cpu, AudioPlan::Aac);
    args.extend(owned(["-c", "copy", "-movflags", "+faststart"]));
    tail(&mut args);
    args
}

pub(crate) fn encode_args(cpu: u32, video: &VideoParams, encode: &Encode) -> Vec<String> {
    let cpu_text = cpu.to_string();
    let mut args = head(cpu, encode.audio);
    args.extend(owned(["-fps_mode", "vfr", "-vf"]));
    args.push(filter(encode));
    // `-threads` before `-i` caps the decoder; here it caps the encoder.
    args.extend(owned([
        "-c:v",
        "libx264",
        "-preset",
        video.preset,
        "-crf",
        &video.crf.to_string(),
        "-profile:v",
        video.profile,
        "-pix_fmt",
        video.pix_fmt,
        "-maxrate",
        video.maxrate,
        "-bufsize",
        video.bufsize,
        "-threads",
        &cpu_text,
        "-colorspace",
        "bt709",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
    ]));
    if encode.audio == AudioPlan::Aac {
        args.extend(owned([
            "-c:a",
            "aac",
            "-b:a",
            video.audio_bitrate,
            "-ac",
            &video.audio_channels.to_string(),
            "-af",
            "aresample=async=1",
        ]));
    }
    if encode.faststart {
        args.extend(owned(["-movflags", "+faststart"]));
    }
    tail(&mut args);
    args
}

/// `setparams` writes the BT.709 tags on the frames: ffmpeg 7 and later take the colour tags of
/// the stream from the filtered frames, so the `-colorspace` family of encoder options alone
/// leaves the transfer and the matrix unspecified. It only tags; the pixel conversion of an SDR
/// input that is not BT.709 is the `convert` step.
const BT709_TAGS: &str = "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709";

fn filter(encode: &Encode) -> String {
    let (width, height) = encode.size;
    let scale = format!("scale={width}:{height}:flags=bicubic");
    if encode.tone_map {
        format!("{scale},{HDR_TO_SDR},{BT709_TAGS}")
    } else if let Some(convert) = &encode.convert {
        format!("{scale},{convert},{BT709_TAGS}")
    } else {
        format!("{scale},{BT709_TAGS}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: VideoParams = VideoParams {
        preset: "veryfast",
        crf: 23,
        profile: "high",
        pix_fmt: "yuv420p",
        maxrate: "8M",
        bufsize: "4M",
        audio_bitrate: "128k",
        audio_channels: 2,
        max_long_edge: 1920,
    };

    fn encode(tone_map: bool, audio: AudioPlan, faststart: bool) -> Encode {
        Encode {
            size: (1920, 1080),
            tone_map,
            audio,
            faststart,
            convert: None,
        }
    }

    fn whitelists() -> Vec<String> {
        input_options()
    }

    #[test]
    fn the_remux_argv_copies_the_first_streams_and_filters_nothing() {
        let args = remux_args(4);

        let mut expected = owned([
            "-nostdin",
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
        ]);
        expected.extend(whitelists());
        expected.extend(owned([
            "-threads",
            "4",
            "-filter_threads",
            "4",
            "-i",
            "/proc/self/fd/9",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-sn",
            "-dn",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
            "result.mp4",
        ]));
        assert_eq!(args, expected);
        assert!(!args.iter().any(|arg| arg == "-vf" || arg == "-af"));
    }

    #[test]
    fn the_sdr_encode_argv_carries_every_flag_of_the_spec() {
        let args = encode_args(4, &VIDEO, &encode(false, AudioPlan::Aac, true));

        let mut expected = owned([
            "-nostdin",
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
        ]);
        expected.extend(whitelists());
        expected.extend(owned([
            "-threads",
            "4",
            "-filter_threads",
            "4",
            "-i",
            "/proc/self/fd/9",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-sn",
            "-dn",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-fps_mode",
            "vfr",
            "-vf",
            "scale=1920:1080:flags=bicubic,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
            "-maxrate",
            "8M",
            "-bufsize",
            "4M",
            "-threads",
            "4",
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ac",
            "2",
            "-af",
            "aresample=async=1",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
            "result.mp4",
        ]));
        assert_eq!(args, expected);
    }

    #[test]
    fn tone_mapping_follows_the_downscale_in_one_filter_chain() {
        let args = encode_args(2, &VIDEO, &encode(true, AudioPlan::Aac, true));

        let at = args.iter().position(|arg| arg == "-vf").expect("-vf");

        assert_eq!(
            args[at + 1],
            format!("scale=1920:1080:flags=bicubic,{HDR_TO_SDR},{BT709_TAGS}")
        );
        assert!(args[at + 1].contains("tonemap=tonemap=hable:desat=0"));
    }

    #[test]
    fn no_audio_drops_the_audio_stream_and_every_audio_option() {
        let args = encode_args(4, &VIDEO, &encode(false, AudioPlan::None, true));

        assert!(args.iter().any(|arg| arg == "-an"));
        assert!(!args.iter().any(|arg| arg == "0:a:0?" || arg == "aac"));
        assert!(!args.iter().any(|arg| arg == "-af" || arg == "-b:a"));
    }

    #[test]
    fn the_index_goes_first_only_when_the_plan_says_faststart() {
        let on = encode_args(4, &VIDEO, &encode(false, AudioPlan::Aac, true));
        let off = encode_args(4, &VIDEO, &encode(false, AudioPlan::Aac, false));

        assert!(on.iter().any(|arg| arg == "+faststart"));
        assert!(!off.iter().any(|arg| arg == "+faststart"));
    }

    #[test]
    fn the_thread_caps_follow_the_cpu_limit_and_no_argv_names_a_job_path() {
        for cpu in [1, 3, 8] {
            let remux = remux_args(cpu);
            let encoded = encode_args(cpu, &VIDEO, &encode(true, AudioPlan::Aac, true));

            for args in [remux, encoded] {
                let value_of = |flag: &str| {
                    args.iter()
                        .position(|arg| arg == flag)
                        .map(|at| args[at + 1].clone())
                };
                assert_eq!(value_of("-threads"), Some(cpu.to_string()));
                assert_eq!(value_of("-filter_threads"), Some(cpu.to_string()));
                assert_eq!(value_of("-i").as_deref(), Some("/proc/self/fd/9"));
                assert_eq!(args.last().map(String::as_str), Some("result.mp4"));
                assert!(args.iter().all(|arg| !arg.contains("/scratch")));
            }
        }
    }

    #[test]
    fn the_conversion_sits_between_the_downscale_and_the_tags() {
        let mut converting = encode(false, AudioPlan::Aac, true);
        converting.convert = Some("zscale=pin=smpte170m:tin=smpte170m:min=smpte170m:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p".to_owned());

        let args = encode_args(4, &VIDEO, &converting);

        let at = args.iter().position(|arg| arg == "-vf").expect("-vf");
        assert_eq!(
            args[at + 1],
            format!(
                "scale=1920:1080:flags=bicubic,{},{BT709_TAGS}",
                converting.convert.as_deref().expect("conversion")
            )
        );
    }
}
