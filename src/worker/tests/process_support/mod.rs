//! The real `sunnycloud-worker` binary against a scripted API listener and two fake decoders, for the tests
//! that must see what the process sends to the API.

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use crate::api_support::Recorded;
use crate::slot_support::{FakeDecoder, ScriptedApi};
use crate::support::{Running, TOKEN};

pub const DAY_SECONDS: u64 = 86_400;
pub const OFFSET_PATH: &str = "/internal/v1/test/clock-offset";

pub struct Process {
    pub api: ScriptedApi,
    pub worker: Running,
    pub seen: Vec<Recorded>,
}

struct Surroundings {
    _decoders: [FakeDecoder; 2],
    _dirs: [TempDir; 3],
}

impl Process {
    /// A worker whose decoders were built `image_age` before now and which reports every 200 ms. Returns once
    /// `/readyz` answers 200. Needs a multi-thread runtime: the fakes serve while the test thread blocks.
    pub async fn start(image_age: Duration) -> Self {
        let sockets = tempfile::tempdir().expect("sockets");
        let interactive_scratch = tempfile::tempdir().expect("scratch");
        let general_scratch = tempfile::tempdir().expect("scratch");
        let gid = fs::metadata(general_scratch.path())
            .expect("metadata")
            .gid();
        let interactive_sock = sockets.path().join("interactive.sock");
        let general_sock = sockets.path().join("general.sock");
        let built = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .saturating_sub(image_age)
            .as_secs();
        let decoders = [
            FakeDecoder::start(&interactive_sock),
            FakeDecoder::start(&general_sock),
        ];
        for decoder in &decoders {
            decoder.built_at(built);
        }
        let api = ScriptedApi::start().await;
        let env = [
            ("WORKER_API_URL", api.base_url()),
            (
                "WORKER_SOCK_INTERACTIVE",
                interactive_sock.display().to_string(),
            ),
            ("WORKER_SOCK_GENERAL", general_sock.display().to_string()),
            (
                "WORKER_SCRATCH_INTERACTIVE",
                interactive_scratch.path().display().to_string(),
            ),
            (
                "WORKER_SCRATCH_GENERAL",
                general_scratch.path().display().to_string(),
            ),
            ("DECODER_GID", gid.to_string()),
            ("WORKER_MAX_INPUT_PHOTO", "16MiB".to_owned()),
            ("WORKER_MAX_INPUT_VIDEO", "16MiB".to_owned()),
            ("WORKER_STATUS_INTERVAL", "200ms".to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (format!("SUNNYCLOUD_{key}"), value))
        .collect();
        let keep = Surroundings {
            _decoders: decoders,
            _dirs: [sockets, interactive_scratch, general_scratch],
        };
        let worker = Running::start(TOKEN, env, Box::new(keep));
        Self {
            api,
            worker,
            seen: Vec::new(),
        }
    }

    /// Moves what the API listener recorded since the last call into `seen`.
    pub fn collect(&mut self) -> &[Recorded] {
        let fresh = self.api.peer.requests();
        self.seen.extend(fresh);
        &self.seen
    }

    /// `image_age_days` of every status report that says `decoder_ok`, oldest first. The report sent before a
    /// decoder has said `hello` carries age 0 by design (09 section 3, step 8) and is left out.
    pub fn status_ages(&mut self) -> Vec<u64> {
        self.collect()
            .iter()
            .filter(|r| r.method == "PUT" && r.path.ends_with("/status"))
            .map(|r| {
                serde_json::from_slice::<serde_json::Value>(&r.body).expect("a JSON status body")
            })
            .filter(|body| body["decoder_ok"] == true)
            .map(|body| body["image_age_days"].as_u64().expect("image_age_days"))
            .collect()
    }

    pub fn offset_polls(&mut self) -> Vec<&Recorded> {
        self.collect();
        self.seen.iter().filter(|r| r.path == OFFSET_PATH).collect()
    }

    /// Waits until `done` holds, or fails after 15 s.
    pub async fn wait_for(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
        let give_up = Instant::now() + Duration::from_secs(15);
        while !done(self) {
            assert!(Instant::now() < give_up, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
