//! A slot loop wired to a scripted API listener and a fake decoder on a real socket, plus the probes and
//! call-log helpers the slot loop tests share.

#![allow(dead_code)]

pub mod api;
pub mod decoder;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::time::Duration;

pub use api::{Entry, ScriptedApi};
pub use decoder::{Answer, FakeDecoder};
use media_shared::decoder_protocol::Limits;
use media_shared::policy::{AudioStream, MediaKind, Probe, VideoStream};
use sc_observability::{BuildInfo, Metrics};
use serde_json::Value;
use sunnycloud_worker::{
    ApiAccess, ApiConfig, Backoff, DecoderMetrics, JobEnd, JobMetrics, KindConfig, LoopConfig,
    ScratchRoot, Slot, SlotConfig, SlotLoop, SlotName, SlotStep, WorkerApiClient,
};
use tempfile::TempDir;

use crate::api_support::{self, Recorded};

pub const MIB: u64 = 1 << 20;
pub const GIB: u64 = 1 << 30;
pub const STALL_KILL_MS: u64 = 90_000;

pub fn photo_limits() -> Limits {
    Limits {
        cpu: 2,
        memory_bytes: 2 * GIB,
        wall: 120_000,
        stall_timeout: STALL_KILL_MS,
        max_output_bytes: 32 * MIB,
    }
}

pub fn video_limits() -> Limits {
    Limits {
        cpu: 4,
        memory_bytes: 4 * GIB,
        wall: 40 * 60_000,
        stall_timeout: STALL_KILL_MS,
        max_output_bytes: 10 * GIB,
    }
}

/// The documented limits, with an input bound a temporary folder can always take.
pub fn loop_config() -> LoopConfig {
    LoopConfig {
        claim_poll_interval: Duration::from_millis(5),
        photo: KindConfig {
            limits: photo_limits(),
            max_input: 64 * MIB,
        },
        video: KindConfig {
            limits: video_limits(),
            max_input: 64 * MIB,
        },
    }
}

pub struct Rig {
    pub decoder: FakeDecoder,
    pub slot_loop: SlotLoop,
    pub jobs: Arc<JobMetrics>,
    pub decoder_metrics: Arc<DecoderMetrics>,
    pub access: ApiAccess,
    scratch: TempDir,
    _sockets: TempDir,
}

impl Rig {
    pub fn new(api: &ScriptedApi, name: SlotName, worker_id: &str) -> Self {
        Self::with_config(api, name, worker_id, loop_config())
    }

    pub fn with_config(
        api: &ScriptedApi,
        name: SlotName,
        worker_id: &str,
        config: LoopConfig,
    ) -> Self {
        let sockets = tempfile::tempdir().expect("socket folder");
        let socket = sockets.path().join("decoder.sock");
        let decoder = FakeDecoder::start(&socket);
        let scratch = tempfile::tempdir().expect("scratch root");
        let gid = fs::metadata(scratch.path()).expect("metadata").gid();
        let metrics = Metrics::new(BuildInfo {
            version: "test",
            commit: "test",
            test_clock: false,
        })
        .expect("metrics");
        let decoder_metrics = DecoderMetrics::register(&metrics).expect("decoder families");
        let jobs = JobMetrics::register(&metrics).expect("job families");
        let slot = Slot::new(
            SlotConfig {
                name,
                socket,
                forbidden_targets: Vec::new(),
                handshake_timeout: Duration::from_secs(5),
                backoff: Backoff::new(Duration::from_millis(5), Duration::from_millis(20))
                    .expect("backoff"),
            },
            Arc::clone(&decoder_metrics),
        );
        let client = WorkerApiClient::new(ApiConfig {
            worker_id: worker_id.to_owned(),
            ..api_support::config(&api.base_url())
        })
        .expect("client");
        let access = ApiAccess::new();
        let slot_loop = SlotLoop::new(
            Arc::new(client),
            access.clone(),
            slot,
            ScratchRoot::new(scratch.path(), gid),
            config,
            Arc::clone(&jobs),
        );
        Self {
            decoder,
            slot_loop,
            jobs,
            decoder_metrics,
            access,
            scratch,
            _sockets: sockets,
        }
    }

    /// Job trees left under the scratch root.
    pub fn scratch_entries(&self) -> usize {
        fs::read_dir(self.scratch.path()).expect("list").count()
    }

    /// One round that must run a job; returns how it ended.
    pub async fn run_one(&mut self) -> JobEnd {
        match self.slot_loop.step().await {
            SlotStep::Ran(end) => end,
            other => panic!("expected a job, got {other:?}"),
        }
    }
}

fn video(width: u32, height: u32, bitrate: u64) -> VideoStream {
    VideoStream {
        codec: Some("h264".into()),
        profile: Some("high".into()),
        width,
        height,
        rotation: None,
        bitrate: Some(bitrate),
        pix_fmt: Some("yuv420p".into()),
        primaries: Some("bt709".into()),
        transfer: Some("bt709".into()),
        matrix: Some("bt709".into()),
    }
}

/// A clip every compliance axis holds for.
pub fn compliant_probe(duration_ms: u64) -> Probe {
    Probe {
        kind: MediaKind::Video,
        container: "mov,mp4,m4a,3gp,3g2,mj2".into(),
        moov_first: true,
        video: Some(video(1920, 1080, 5_000_000)),
        audio: Some(AudioStream {
            codec: "aac".into(),
        }),
        duration_ms: Some(duration_ms),
    }
}

/// Compliant except that the index is at the end.
pub fn remux_probe() -> Probe {
    Probe {
        moov_first: false,
        ..compliant_probe(60_000)
    }
}

/// Over the bitrate bound: needs a transcode.
pub fn transcode_probe() -> Probe {
    Probe {
        video: Some(video(1920, 1080, 12_000_000)),
        ..compliant_probe(60_000)
    }
}

pub fn photo_probe() -> Probe {
    Probe {
        kind: MediaKind::Photo,
        container: "jpeg".into(),
        moov_first: false,
        video: Some(VideoStream {
            codec: None,
            profile: None,
            width: 4000,
            height: 3000,
            rotation: None,
            bitrate: None,
            pix_fmt: None,
            primaries: None,
            transfer: None,
            matrix: None,
        }),
        audio: None,
        duration_ms: None,
    }
}

/// One line per request: `claim image`, `input`, `output preview`, `result`, `failure`, `lease`.
pub fn calls(requests: &[Recorded]) -> Vec<String> {
    requests.iter().map(describe).collect()
}

fn describe(request: &Recorded) -> String {
    let route: Vec<&str> = request
        .path
        .trim_start_matches("/api/v1/worker-jobs")
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    match route.as_slice() {
        ["claims"] => format!("claim {}", json(request)["lane"].as_str().unwrap_or("?")),
        [_, "outputs", kind] => format!("output {kind}"),
        [_, tail] => (*tail).to_owned(),
        other => format!("{} {other:?}", request.method),
    }
}

pub fn json(request: &Recorded) -> Value {
    serde_json::from_slice(&request.body).expect("a JSON body")
}

/// The lanes of the claims in `requests`, in order.
pub fn claimed_lanes(requests: &[Recorded]) -> Vec<String> {
    requests
        .iter()
        .filter(|request| request.path.ends_with("/claims"))
        .map(|request| json(request)["lane"].as_str().unwrap_or("?").to_owned())
        .collect()
}
