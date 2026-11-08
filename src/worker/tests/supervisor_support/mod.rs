//! The whole supervisor in process: `run_worker` on short settings, against a scripted API listener and two
//! fake decoders on real sockets, with a manual clock.

#![allow(dead_code)]

use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_clock::{Clock, ManualClock, Timestamp};
use sc_observability::{BuildInfo, Metrics};
use sunnycloud_worker::{WorkerError, load_settings, run_worker};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::api_support::Recorded;
use crate::slot_support::{FakeDecoder, ScriptedApi, wait_until};
use crate::support::{TOKEN, TokenFile, free_port, http_get, token_env};

pub const BUILT: i64 = 1_700_000_000;
pub const DAY_SECONDS: i64 = 86_400;

/// Settings in milliseconds, so a run takes well under a second per step.
pub struct Timing {
    pub status_interval: Duration,
    pub shutdown_grace: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            status_interval: Duration::from_millis(200),
            shutdown_grace: Duration::from_millis(600),
        }
    }
}

pub struct Harness {
    pub api: ScriptedApi,
    pub interactive: FakeDecoder,
    pub general: FakeDecoder,
    pub interactive_root: PathBuf,
    pub general_root: PathBuf,
    pub ops: SocketAddr,
    pub metrics: Arc<Metrics>,
    pub seen: Vec<Recorded>,
    env: std::collections::BTreeMap<String, String>,
    _dirs: (TempDir, TempDir, TempDir),
    _token: TokenFile,
}

pub struct Running {
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), WorkerError>>,
}

impl Running {
    /// SIGTERM, then the result of the supervisor.
    pub async fn stop(self) -> Result<(), WorkerError> {
        self.stop.send(()).expect("the supervisor is running");
        self.task.await.expect("the supervisor task")
    }

    /// SIGTERM, then the result of the supervisor, which must end within `within`.
    pub async fn ended_after_stop(self, within: Duration) -> Result<(), WorkerError> {
        self.stop.send(()).expect("the supervisor is running");
        tokio::time::timeout(within, self.task)
            .await
            .expect("the supervisor ended in time")
            .expect("the supervisor task")
    }
}

impl Harness {
    pub async fn new(timing: &Timing) -> Self {
        let interactive_dir = tempfile::tempdir().expect("scratch");
        let general_dir = tempfile::tempdir().expect("scratch");
        let sockets = tempfile::tempdir().expect("sockets");
        let gid = fs::metadata(general_dir.path()).expect("metadata").gid();
        let interactive_sock = sockets.path().join("interactive.sock");
        let general_sock = sockets.path().join("general.sock");
        let interactive = FakeDecoder::start(&interactive_sock);
        let general = FakeDecoder::start(&general_sock);
        let api = ScriptedApi::start().await;
        let token = TokenFile::new(TOKEN);
        let ops: SocketAddr = ([127, 0, 0, 1], free_port()).into();
        let mut env = token_env(&token);
        let mut set = |key: &str, value: String| {
            env.insert(format!("SUNNYCLOUD_{key}"), value);
        };
        set("WORKER_API_URL", api.base_url());
        set("WORKER_OPS_LISTEN", ops.to_string());
        set(
            "WORKER_SOCK_INTERACTIVE",
            interactive_sock.display().to_string(),
        );
        set("WORKER_SOCK_GENERAL", general_sock.display().to_string());
        set(
            "WORKER_SCRATCH_INTERACTIVE",
            interactive_dir.path().display().to_string(),
        );
        set(
            "WORKER_SCRATCH_GENERAL",
            general_dir.path().display().to_string(),
        );
        set("DECODER_GID", gid.to_string());
        // The input bound must fit the free space of a temporary folder.
        set("WORKER_MAX_INPUT_PHOTO", "16MiB".to_owned());
        set("WORKER_MAX_INPUT_VIDEO", "16MiB".to_owned());
        set("WORKER_CLAIM_POLL_INTERVAL", "5ms".to_owned());
        set(
            "WORKER_STATUS_INTERVAL",
            format!("{}ms", timing.status_interval.as_millis()),
        );
        set(
            "WORKER_SHUTDOWN_GRACE",
            format!("{}ms", timing.shutdown_grace.as_millis()),
        );
        set("WORKER_ID", "supervisor-under-test".to_owned());
        let metrics = Arc::new(
            Metrics::new(BuildInfo {
                version: "test",
                commit: "test",
                test_clock: false,
            })
            .expect("metrics"),
        );
        Self {
            api,
            interactive,
            general,
            interactive_root: interactive_dir.path().to_owned(),
            general_root: general_dir.path().to_owned(),
            ops,
            metrics,
            seen: Vec::new(),
            env,
            _dirs: (interactive_dir, general_dir, sockets),
            _token: token,
        }
    }

    /// The supervisor on a clock 20 days after the decoders' build.
    pub fn start(&self) -> Running {
        let settings = load_settings(self.env.clone()).expect("the test settings load");
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Timestamp::from_unix_micros(
            (BUILT + 20 * DAY_SECONDS) * 1_000_000,
        )));
        let (stop, stopped) = oneshot::channel::<()>();
        let metrics = Arc::clone(&self.metrics);
        let task = tokio::spawn(run_worker(settings, metrics, clock, async move {
            stopped.await.expect("the stop signal is sent");
        }));
        Running { stop, task }
    }

    /// Moves what the API listener recorded since the last call into `seen`.
    pub fn collect(&mut self) -> &[Recorded] {
        let fresh = self.api.peer.requests();
        self.seen.extend(fresh);
        &self.seen
    }

    pub async fn wait_for_request(&mut self, what: impl Fn(&Recorded) -> bool) {
        let give_up = Instant::now() + Duration::from_secs(10);
        while !self.collect().iter().any(&what) {
            assert!(Instant::now() < give_up, "the request never arrived");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    pub async fn wait_for_statuses(&mut self, count: usize) {
        let give_up = Instant::now() + Duration::from_secs(10);
        while self.statuses().len() < count {
            assert!(Instant::now() < give_up, "too few status reports");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn statuses(&mut self) -> Vec<&Recorded> {
        self.collect();
        self.seen.iter().filter(|r| is_status(r)).collect()
    }

    pub async fn get(&self, path: &'static str) -> (u16, String) {
        let addr = self.ops;
        tokio::task::spawn_blocking(move || http_get(addr, path))
            .await
            .expect("request task")
    }

    /// Polls `/readyz` until the listener answers (the supervisor binds it after the sweep).
    pub async fn wait_for_ops(&self) {
        let addr = self.ops;
        wait_until(|| std::net::TcpStream::connect(addr).is_ok()).await;
    }
}

pub fn is_status(request: &Recorded) -> bool {
    request.method == "PUT" && request.path.ends_with("/status")
}

pub fn body(request: &Recorded) -> serde_json::Value {
    serde_json::from_slice(&request.body).expect("a JSON body")
}
