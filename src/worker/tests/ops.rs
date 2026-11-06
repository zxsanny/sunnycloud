//! AC-3 in process: the ops router the worker serves, with readiness checks registered the way the slot tasks
//! will register theirs.

use std::net::SocketAddr;
use std::sync::Arc;

use sc_observability::{
    BuildInfo, CheckFuture, Liveness, Metrics, ReadinessCheck, ReadinessItem, ReadinessRegistry,
    ops_router,
};
use sunnycloud_worker::{ProbeError, get_ok, serve_ops};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

mod support;

struct Failing;

impl ReadinessCheck for Failing {
    fn name(&self) -> &str {
        "decoder_interactive"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async { ReadinessItem::failing("socket not reachable") })
    }
}

struct Ops {
    addr: SocketAddr,
    liveness: Liveness,
    stop: oneshot::Sender<()>,
    server: JoinHandle<()>,
}

async fn start(readiness: ReadinessRegistry) -> Ops {
    let metrics = Arc::new(
        Metrics::new(BuildInfo {
            version: "0.0.0",
            commit: "test",
            test_clock: false,
        })
        .expect("registry"),
    );
    let liveness = Liveness::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let addr = listener.local_addr().expect("address");
    let (stop, stopped) = oneshot::channel::<()>();
    let router = ops_router(metrics, readiness, liveness.clone());
    let server = tokio::spawn(async move {
        serve_ops(listener, router, async {
            stopped.await.expect("stop signal is sent");
        })
        .await
        .expect("serves");
    });
    Ops {
        addr,
        liveness,
        stop,
        server,
    }
}

impl Ops {
    async fn get(&self, path: &'static str) -> (u16, String) {
        let addr = self.addr;
        tokio::task::spawn_blocking(move || support::http_get(addr, path))
            .await
            .expect("request task")
    }

    async fn shut_down(self) {
        self.stop.send(()).expect("server is running");
        self.server.await.expect("server task");
    }
}

#[tokio::test]
async fn ac3_without_failing_checks_all_three_endpoints_answer_200() {
    let ops = start(ReadinessRegistry::new()).await;

    let (healthz, _) = ops.get("/healthz").await;
    let (readyz, _) = ops.get("/readyz").await;
    let (metrics, body) = ops.get("/metrics").await;

    assert_eq!((healthz, readyz, metrics), (200, 200, 200));
    let names: Vec<&str> = body
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split(['{', ' ']).next().expect("name"))
        .collect();
    assert!(!names.is_empty(), "no samples in {body}");
    for name in names {
        assert!(name.starts_with("sunnycloud_"), "{name} lacks the prefix");
    }
    ops.shut_down().await;
}

#[tokio::test]
async fn ac3_a_failing_check_turns_readyz_to_503_naming_it_and_leaves_healthz_alone() {
    let mut readiness = ReadinessRegistry::new();
    readiness.add(Arc::new(Failing));
    let ops = start(readiness).await;

    let (healthz, _) = ops.get("/healthz").await;
    let (readyz, body) = ops.get("/readyz").await;

    assert_eq!(healthz, 200);
    assert_eq!(readyz, 503);
    assert!(body.contains("decoder_interactive"), "{body}");
    assert!(!body.contains(r#""status":"ok""#), "{body}");
    ops.shut_down().await;
}

#[tokio::test]
async fn ac3_readyz_turns_503_once_shutdown_starts() {
    let ops = start(ReadinessRegistry::new()).await;

    ops.liveness.shut_down();
    let (readyz, _) = ops.get("/readyz").await;

    assert_eq!(readyz, 503);
    ops.shut_down().await;
}

#[tokio::test]
async fn ac4_the_probe_accepts_200_and_rejects_503() {
    let ready = start(ReadinessRegistry::new()).await;
    let mut failing = ReadinessRegistry::new();
    failing.add(Arc::new(Failing));
    let unready = start(failing).await;

    let accepted = get_ok(ready.addr, "/readyz").await;
    let rejected = get_ok(unready.addr, "/readyz").await;

    assert!(accepted.is_ok(), "{accepted:?}");
    assert!(
        matches!(rejected, Err(ProbeError::Status { .. })),
        "{rejected:?}"
    );
    ready.shut_down().await;
    unready.shut_down().await;
}

#[tokio::test]
async fn ac4_the_probe_fails_when_nothing_listens() {
    let addr: SocketAddr = ([127, 0, 0, 1], support::free_port()).into();

    let result = get_ok(addr, "/readyz").await;

    assert!(matches!(result, Err(ProbeError::Io { .. })), "{result:?}");
}
