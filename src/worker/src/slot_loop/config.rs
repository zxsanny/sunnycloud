use std::time::Duration;

use media_shared::decoder_protocol::Limits;
use media_shared::types::JobKind;
use sc_config::WorkerSettings;

use crate::settings::NVENC_ON;

/// What the loop sends the decoder and checks before a fetch for one kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindConfig {
    pub limits: Limits,
    /// `worker_max_input_photo` or `worker_max_input_video`: the most the scratch mount must be able to take.
    pub max_input: u64,
}

impl KindConfig {
    /// `photo_job_timeout` or `video_job_timeout`: the longest one decoder call of this kind may take.
    pub fn wall(&self) -> Duration {
        Duration::from_millis(self.limits.wall)
    }
}

/// The settings the slot loop reads. Every duration comes from the settings, so tests run with short ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopConfig {
    /// `worker_claim_poll_interval`: the wait after a claim round that found nothing.
    pub claim_poll_interval: Duration,
    /// `decoder_silence_timeout`: tier 2, no frame of any kind from the decoder for this long.
    pub silence_timeout: Duration,
    /// `decoder_cancel_grace`: how long a cancelled decoder gets to answer before the slot is unhealthy.
    pub cancel_grace: Duration,
    pub photo: KindConfig,
    pub video: KindConfig,
    /// `nvenc = on`: a transcode is planned for the GPU encoder when the slot's decoder also proved it.
    pub nvenc: bool,
}

impl LoopConfig {
    pub fn from_settings(settings: &WorkerSettings) -> Self {
        let stall_timeout = millis(settings.decoder_stall_kill.get());
        Self {
            claim_poll_interval: settings.worker_claim_poll_interval.get(),
            silence_timeout: settings.decoder_silence_timeout.get(),
            cancel_grace: settings.decoder_cancel_grace.get(),
            photo: KindConfig {
                limits: Limits {
                    cpu: cpu(settings.photo_cpu),
                    memory_bytes: settings.photo_memory.bytes(),
                    wall: millis(settings.photo_job_timeout.get()),
                    stall_timeout,
                    max_output_bytes: settings.decoder_max_output_photo.bytes(),
                },
                max_input: settings.worker_max_input_photo.bytes(),
            },
            video: KindConfig {
                limits: Limits {
                    cpu: cpu(settings.video_cpu),
                    memory_bytes: settings.video_memory.bytes(),
                    wall: millis(settings.video_job_timeout.get()),
                    stall_timeout,
                    max_output_bytes: settings.decoder_max_output_video.bytes(),
                },
                max_input: settings.worker_max_input_video.bytes(),
            },
            nvenc: settings.nvenc == NVENC_ON,
        }
    }

    /// `stills` jobs run under the photo limits, `poster` and `transcode` under the video limits.
    pub fn for_kind(&self, kind: JobKind) -> &KindConfig {
        match kind {
            JobKind::Stills => &self.photo,
            JobKind::Poster | JobKind::Transcode => &self.video,
        }
    }
}

/// A value above the type maximum is sent as the maximum: the protocol accepts anything up to it and the
/// runner clamps to its own ceiling.
fn cpu(count: u64) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
