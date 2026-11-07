//! Mode S of `components/11_http_api/tests.md`: the real server over real TCP, with every port a scripted
//! stub. A call nothing scripted fails as `Unexpected`, never as a default success.

pub mod checks;
pub mod clients;
pub mod probe;
pub mod raw_h2;
pub mod router_server;
pub mod script;
mod stubs;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sc_observability::{BuildInfo, Metrics, ReadinessCheck};
use sunnycloud_http_api::{
    AcceptLoop, ApiServices, HttpConfig, HttpMetrics, HttpServer, HttpServerError, ServerInfo,
};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub use script::Script;

use stubs::*;

/// Longest a test waits for an event it expects; a bound for failure, never a sleep.
pub const EVENT_BOUND: Duration = Duration::from_secs(10);

/// The defaults of `common-helpers/01_helper_config.md`, on ephemeral loopback ports.
pub fn default_config() -> HttpConfig {
    let loopback: SocketAddr = ([127, 0, 0, 1], 0).into();
    HttpConfig {
        public_url: "https://sunnycloud.test".to_owned(),
        trusted_proxies: Vec::new(),
        public_addr: loopback,
        internal_addr: loopback,
        ops_addr: loopback,
        max_connections: 4096,
        h1_header_timeout: Duration::from_secs(10),
        h2_stream_window: 4 << 20,
        h2_conn_window: 32 << 20,
        h2_max_streams: 100,
        h2_max_pending_reset: 20,
        h2_max_header_list: 16 << 10,
        h2_keepalive_interval: Duration::from_secs(30),
        h2_keepalive_timeout: Duration::from_secs(20),
        request_timeout: Duration::from_secs(60),
        body_idle_timeout: Duration::from_secs(30),
        shutdown_grace: Duration::from_secs(25),
        min_client_version: None,
        server_info: ServerInfo::default(),
    }
}

pub fn new_metrics() -> Arc<Metrics> {
    let build = BuildInfo {
        version: "test",
        commit: "test",
        test_clock: false,
    };
    Arc::new(Metrics::new(build).expect("metrics registry"))
}

/// An `ApiServices` whose every port is a stub reading `script`.
pub fn stub_services(script: &Script, metrics: Arc<Metrics>) -> ApiServices {
    #[cfg(feature = "test-clock")]
    let offset_clock = Arc::new(sc_clock::OffsetClock::new());
    ApiServices {
        authenticator: AuthenticatorStub::new(script),
        principal_revalidator: PrincipalRevalidatorStub::new(script),
        sign_in: SignInStub::new(script),
        sessions: SessionStub::new(script),
        device_grants: DeviceGrantStub::new(script),
        invites: InviteStub::new(script),
        tree: TreeStub::new(script),
        tree_reader: TreeReaderStub::new(script),
        uploads: UploadStub::new(script),
        commits: CommitStub::new(script),
        downloads: DownloadStub::new(script),
        change_feed: ChangeFeedStub::new(script),
        shares: ShareStub::new(script),
        public_shares: PublicShareStub::new(script),
        admin: AdminStub::new(script),
        accounts: AccountStub::new(script),
        account_deletion: AccountDeletionStub::new(script),
        thumbnail_states: ThumbnailStateStub::new(script),
        derived: DerivedStub::new(script),
        worker_auth: WorkerAuthStub::new(script),
        worker_jobs: WorkerJobsStub::new(script),
        vault: Some(VaultStub::new(script)),
        readiness: Vec::new(),
        metrics,
        #[cfg(feature = "test-clock")]
        clock: offset_clock.clone(),
        #[cfg(not(feature = "test-clock"))]
        clock: Arc::new(sc_clock::SystemClock::new()),
        #[cfg(feature = "test-clock")]
        offset_clock,
    }
}

/// The value of the sample line `name` (with labels) in a scrape; `None` when the series is absent.
pub async fn metric(metrics: &Metrics, name: &str) -> Option<f64> {
    let text = metrics.render().await.expect("render metrics");
    text.lines().find_map(|line| {
        line.strip_prefix(name)?
            .strip_prefix(' ')?
            .trim()
            .parse()
            .ok()
    })
}

/// Polls until `condition` holds; panics after [`EVENT_BOUND`].
pub async fn wait_until<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let outcome = tokio::time::timeout(EVENT_BOUND, async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(outcome.is_ok(), "timed out waiting for {what}");
}

/// `HttpServer::run` on ephemeral ports.
pub struct Server {
    pub public: SocketAddr,
    pub internal: SocketAddr,
    pub ops: SocketAddr,
    pub script: Script,
    pub metrics: Arc<Metrics>,
    pub http_metrics: Arc<HttpMetrics>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<(), HttpServerError>>,
}

impl Server {
    pub async fn start(config: HttpConfig) -> Server {
        Server::start_with_readiness(config, Vec::new()).await
    }

    /// Starts a server whose `/readyz` runs `readiness`.
    pub async fn start_with_readiness(
        config: HttpConfig,
        readiness: Vec<Arc<dyn ReadinessCheck>>,
    ) -> Server {
        let script = Script::default();
        let metrics = new_metrics();
        let services = ApiServices {
            readiness,
            ..stub_services(&script, metrics.clone())
        };
        let server = HttpServer::new(services, config).expect("server builds");
        let http_metrics = server.http_metrics();
        let bound = server.bind().await.expect("listeners bind");
        let (public, internal, ops) =
            (bound.public_addr(), bound.internal_addr(), bound.ops_addr());
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(bound.serve(async move {
            signal.await.ok();
        }));
        Server {
            public,
            internal,
            ops,
            script,
            metrics,
            http_metrics,
            shutdown,
            task,
        }
    }

    /// Sends the shutdown signal and returns what `run` returned.
    pub async fn stop(self) -> Result<(), HttpServerError> {
        self.shutdown.send(()).expect("server is still running");
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("run returns after the signal")
            .expect("run does not panic")
    }
}

/// The shared accept loop serving [`probe::router`], whose routes call the stub ports.
pub struct Probe {
    pub addr: SocketAddr,
    pub script: Script,
    pub metrics: Arc<Metrics>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Probe {
    pub async fn start(config: HttpConfig) -> Probe {
        let script = Script::default();
        let metrics = new_metrics();
        let services = stub_services(&script, metrics.clone());
        let accept = AcceptLoop::new(&config, &metrics).expect("accept loop builds");
        let listener = TcpListener::bind(config.public_addr)
            .await
            .expect("probe binds");
        let addr = listener.local_addr().expect("probe address");
        let stop = CancellationToken::new();
        let task = tokio::spawn({
            let stop = stop.clone();
            async move { accept.serve(listener, probe::router(&services), stop).await }
        });
        Probe {
            addr,
            script,
            metrics,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        self.stop.cancel();
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("the accept loop stops")
            .expect("the accept loop does not panic");
    }
}
