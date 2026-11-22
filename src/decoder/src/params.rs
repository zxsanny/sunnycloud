//! The decoder's `Params` table: the quality values of each `params_version`. A request names a
//! version (`render_still.params_version`, `Plan::Transcode.params_version`) and the decoder
//! looks it up here; the values ship in this binary and never come from the environment
//! (10 section 4, decoder protocol open point 7, decided 2026-11-22).
//!
//! A version this build has no table for is `DecodeError::ProtocolMismatch`: the supervisor and
//! the decoder image disagree on the version tables, which is a clean error, not a violation.
//!
//! Changing any value needs a new version. The CRF and preset of version 1 are the numbers of
//! the AZ-3860 spec, not tuned: tuning them so that derived storage stays within 20 % of the
//! originals (AC-44, NFT-PERF-20) is open point 4 of `worker_job_protocol.md`.

use media_shared::decoder_protocol::DecodeError;

/// The x264 and AAC settings of a CPU transcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VideoParams {
    pub preset: &'static str,
    pub crf: u32,
    pub profile: &'static str,
    pub pix_fmt: &'static str,
    pub maxrate: &'static str,
    pub bufsize: &'static str,
    pub audio_bitrate: &'static str,
    pub audio_channels: u32,
    /// The long edge of the output never exceeds this, whatever the plan's short edge says.
    pub max_long_edge: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Params {
    pub webp_quality: u32,
    pub video: VideoParams,
}

/// `webp_quality` is the value the runner has used since AZ-3857; no contract table states a
/// number (`worker_job_protocol.md` "Output parameters" names the sizes only).
static VERSION_1: Params = Params {
    webp_quality: 80,
    video: VideoParams {
        preset: "veryfast",
        crf: 23,
        profile: "high",
        pix_fmt: "yuv420p",
        maxrate: "8M",
        bufsize: "4M",
        audio_bitrate: "128k",
        audio_channels: 2,
        max_long_edge: 1920,
    },
};

pub(crate) fn for_version(params_version: u32) -> Result<&'static Params, DecodeError> {
    match params_version {
        1 => Ok(&VERSION_1),
        _ => Err(DecodeError::ProtocolMismatch),
    }
}

#[cfg(test)]
mod tests {
    use media_shared::policy::{CURRENT_PARAMS_VERSION, Params as Policy};

    use super::*;

    #[test]
    fn version_1_holds_the_values_of_the_transcode_spec() {
        let params = for_version(1).expect("version 1");

        assert_eq!(params.webp_quality, 80);
        assert_eq!(
            params.video,
            VideoParams {
                preset: "veryfast",
                crf: 23,
                profile: "high",
                pix_fmt: "yuv420p",
                maxrate: "8M",
                bufsize: "4M",
                audio_bitrate: "128k",
                audio_channels: 2,
                max_long_edge: 1920,
            }
        );
    }

    #[test]
    fn a_version_without_a_table_is_a_protocol_mismatch() {
        for version in [0, 2, 99, u32::MAX] {
            let result = for_version(version);

            assert_eq!(result, Err(DecodeError::ProtocolMismatch), "{version}");
        }
    }

    #[test]
    fn the_long_edge_cap_equals_the_policy_cap_of_the_same_version() {
        let policy = Policy::for_version(CURRENT_PARAMS_VERSION).expect("policy table");

        let cap = for_version(CURRENT_PARAMS_VERSION)
            .expect("decoder table")
            .video
            .max_long_edge;

        assert_eq!(cap, policy.video.max_long_side);
    }
}
