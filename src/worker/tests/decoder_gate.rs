//! AZ-3871 AC-1 (start-up gate) and AC-3 (reconnect order and backoff). The decoder is the real runner
//! socket loop of the `decoder-runner` crate with a scripted handler, or a fake peer on a real socket.

mod decoder_support;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use decoder_support::{AcceptAndClose, Handler, HostilePeer, Runner, eventually, frame, within};
use media_shared::decoder_protocol::{Response, SelfCheckItem};
use sc_observability::{BuildInfo, Liveness, Metrics, ReadinessRegistry, ops_router};
use serde_json::json;
use sunnycloud_worker::{
    Backoff, DecoderMetrics, GateFailure, Slot, SlotConfig, SlotName, SlotState, get_ok, serve_ops,
};
use tempfile::TempDir;
use tokio::net::TcpListener;

const FORBIDDEN: [&str; 5] = [
    "api:8080",
    "api:8081",
    "db:5432",
    "1.1.1.1:53",
    "example.com:443",
];
const ALIVE: Duration = Duration::from_millis(20);
const WAIT: Duration = Duration::from_secs(10);

struct Harness {
    _dir: TempDir,
    socket: PathBuf,
    metrics: Arc<Metrics>,
    decoder: Arc<DecoderMetrics>,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temporary folder");
        let metrics = Arc::new(
            Metrics::new(BuildInfo {
                version: "0",
                commit: "0",
                test_clock: false,
            })
            .expect("metrics"),
        );
        let decoder = DecoderMetrics::register(&metrics).expect("decoder families");
        Self {
            socket: dir.path().join("decoder.sock"),
            _dir: dir,
            metrics,
            decoder,
        }
    }

    fn slot(&self, backoff: Backoff) -> Slot {
        Slot::new(
            SlotConfig {
                name: SlotName::Interactive,
                socket: self.socket.clone(),
                forbidden_targets: FORBIDDEN.map(str::to_owned).to_vec(),
                handshake_timeout: Duration::from_secs(2),
                backoff,
            },
            Arc::clone(&self.decoder),
        )
    }

    /// `/readyz` of an ops listener with the slot's readiness check registered.
    async fn ops(&self, slot: &Slot) -> SocketAddr {
        let mut readiness = ReadinessRegistry::new();
        readiness.add(Arc::new(slot.readiness()));
        let router = ops_router(Arc::clone(&self.metrics), readiness, Liveness::new());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        tokio::spawn(serve_ops(listener, router, std::future::pending()));
        addr
    }
}

fn fast_backoff() -> Backoff {
    Backoff::new(Duration::from_millis(10), Duration::from_millis(80)).expect("backoff")
}

fn failed_item() -> Vec<SelfCheckItem> {
    vec![
        SelfCheckItem {
            name: "no_secrets".into(),
            ok: true,
        },
        SelfCheckItem {
            name: "no_network".into(),
            ok: false,
        },
    ]
}

#[tokio::test]
async fn all_ok_self_check_makes_the_slot_ready() {
    // Arrange
    let harness = Harness::new();
    let handler = Handler::new(Response::Cancelled);
    let _runner = Runner::start(&harness.socket, &handler, ALIVE);
    let mut slot = harness.slot(fast_backoff());
    let ops = harness.ops(&slot).await;
    let state = slot.subscribe();

    // Act
    let gated = within(WAIT, slot.next_ready())
        .await
        .expect("the gate passes");

    // Assert
    assert!(state.borrow().is_ready());
    assert!(state.borrow().decoder_ok());
    assert_eq!(gated.capabilities.build_timestamp, 1_700_000_000);
    assert_eq!(state.borrow().capabilities(), Some(&gated.capabilities));
    assert!(harness.decoder.selfcheck_ok(SlotName::Interactive));
    get_ok(ops, "/readyz").await.expect("/readyz answers 200");
    let scrape = harness.metrics.render().await.expect("render");
    assert!(scrape.contains("sunnycloud_worker_selfcheck_ok{slot=\"interactive\"} 1"));
    assert_eq!(handler.log(), ["hello", "self_check"]);
    let sent = handler.targets.lock().expect("targets").clone();
    assert_eq!(sent, vec![FORBIDDEN.map(str::to_owned).to_vec()]);
}

#[tokio::test]
async fn one_failed_item_keeps_the_slot_out_of_service_and_readyz_503() {
    // Arrange
    let harness = Harness::new();
    let handler = Handler::new(Response::Cancelled);
    *handler.items.lock().expect("items") = failed_item();
    let _runner = Runner::start(&harness.socket, &handler, ALIVE);
    let mut slot = harness.slot(fast_backoff());
    let ops = harness.ops(&slot).await;
    let state = slot.subscribe();

    // Act
    let gated = within(Duration::from_millis(400), slot.next_ready()).await;

    // Assert
    assert!(
        gated.is_none(),
        "a failed item must not hand out a connection"
    );
    assert_eq!(
        *state.borrow(),
        SlotState::GateFailed(GateFailure::Items { failed: 1 })
    );
    assert!(!state.borrow().decoder_ok());
    assert!(!harness.decoder.selfcheck_ok(SlotName::Interactive));
    assert!(
        get_ok(ops, "/readyz").await.is_err(),
        "/readyz must answer 503"
    );
    let scrape = harness.metrics.render().await.expect("render");
    assert!(scrape.contains("sunnycloud_worker_selfcheck_ok{slot=\"interactive\"} 0"));
    assert!(
        handler.log().contains(&"self_check"),
        "the gate must have asked"
    );
    assert!(!handler.log().contains(&"job"));
}

#[tokio::test]
async fn an_empty_self_check_report_proves_nothing_and_fails_the_gate() {
    // Arrange
    let harness = Harness::new();
    let handler = Handler::new(Response::Cancelled);
    handler.items.lock().expect("items").clear();
    let _runner = Runner::start(&harness.socket, &handler, ALIVE);
    let mut slot = harness.slot(fast_backoff());
    let state = slot.subscribe();

    // Act
    let gated = within(Duration::from_millis(300), slot.next_ready()).await;

    // Assert
    assert!(gated.is_none());
    assert_eq!(
        *state.borrow(),
        SlotState::GateFailed(GateFailure::Items { failed: 1 })
    );
}

#[tokio::test]
async fn hello_answered_with_another_protocol_fails_the_gate() {
    // Arrange
    let harness = Harness::new();
    let capabilities = json!({"response": {"capabilities": {
        "protocol": 2, "vips": "v", "ffmpeg": "f", "libheif": "h",
        "build_timestamp": 1, "nvenc_probe_ok": false, "nvenc_sessions": 0}}});
    let _peer = HostilePeer::start(&harness.socket, frame(capabilities.to_string().as_bytes()));
    let mut slot = harness.slot(fast_backoff());
    let state = slot.subscribe();

    // Act
    let gated = within(Duration::from_millis(300), slot.next_ready()).await;

    // Assert
    assert!(gated.is_none());
    assert_eq!(*state.borrow(), SlotState::GateFailed(GateFailure::Hello));
}

#[tokio::test]
async fn a_decoder_that_never_answers_the_handshake_fails_the_gate_by_timeout() {
    // Arrange
    let harness = Harness::new();
    let _peer = HostilePeer::start(&harness.socket, Vec::new());
    let mut slot = Slot::new(
        SlotConfig {
            name: SlotName::General,
            socket: harness.socket.clone(),
            forbidden_targets: Vec::new(),
            handshake_timeout: Duration::from_millis(100),
            backoff: fast_backoff(),
        },
        Arc::clone(&harness.decoder),
    );
    let state = slot.subscribe();

    // Act
    let gated = within(Duration::from_millis(500), slot.next_ready()).await;

    // Assert
    assert!(gated.is_none());
    assert_eq!(*state.borrow(), SlotState::GateFailed(GateFailure::Silent));
}

#[tokio::test]
async fn after_a_runner_exit_the_new_connection_repeats_hello_then_self_check_and_counts_one_restart()
 {
    // Arrange
    let harness = Harness::new();
    let handler = Handler::new(Response::Cancelled);
    let runner = Runner::start(&harness.socket, &handler, ALIVE);
    let mut slot = harness.slot(fast_backoff());
    let state = slot.subscribe();
    let first = within(WAIT, slot.next_ready()).await.expect("first gate");
    assert_eq!(harness.decoder.restarts(SlotName::Interactive), 0);

    // Act: the runner exits while idle and stays away while the client keeps trying.
    drop(first);
    runner.exit().expect("the runner stops cleanly");
    let reconnect = tokio::spawn(async move { slot.next_ready().await });
    assert!(!eventually(Duration::from_millis(300), || reconnect.is_finished()).await);
    let during_outage = (state.borrow().clone(), handler.log());
    let _runner = Runner::start(&harness.socket, &handler, ALIVE);
    let second = within(WAIT, reconnect)
        .await
        .expect("second gate")
        .expect("task");

    // Assert
    assert_eq!(during_outage.0, SlotState::Connecting);
    assert_eq!(
        during_outage.1,
        ["hello", "self_check"],
        "nothing reached the dead runner"
    );
    assert_eq!(
        handler.log(),
        ["hello", "self_check", "hello", "self_check"]
    );
    assert!(state.borrow().is_ready());
    assert_eq!(harness.decoder.restarts(SlotName::Interactive), 1);
    assert!(second.conn.is_open());
}

#[tokio::test]
async fn reconnect_attempts_wait_at_least_the_backoff_and_never_less_than_before() {
    // Arrange
    let harness = Harness::new();
    let peer = AcceptAndClose::start(&harness.socket);
    let backoff =
        Backoff::new(Duration::from_millis(20), Duration::from_millis(80)).expect("backoff");
    let mut slot = harness.slot(backoff);

    // Act
    let gated = within(Duration::from_millis(800), slot.next_ready()).await;

    // Assert
    assert!(gated.is_none());
    let attempts = peer.accepted();
    assert!(attempts.len() >= 5, "only {} attempts", attempts.len());
    let gaps: Vec<Duration> = attempts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let mut expected = backoff.initial();
    for (index, gap) in gaps.iter().enumerate() {
        assert!(
            *gap >= expected,
            "gap {index} was {gap:?}, below the backoff {expected:?}"
        );
        expected = backoff.after(expected);
    }
}

#[test]
fn backoff_doubles_and_stops_at_its_bound() {
    // Arrange
    let backoff =
        Backoff::new(Duration::from_millis(100), Duration::from_millis(700)).expect("backoff");

    // Act
    let mut delay = backoff.initial();
    let mut sequence = vec![delay];
    for _ in 0..6 {
        delay = backoff.after(delay);
        sequence.push(delay);
    }

    // Assert
    let millis: Vec<u128> = sequence.iter().map(Duration::as_millis).collect();
    assert_eq!(millis, [100, 200, 400, 700, 700, 700, 700]);
    assert!(sequence.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn backoff_bounds_that_cannot_work_are_refused() {
    // Arrange
    let one = Duration::from_millis(1);

    // Act
    let zero_initial = Backoff::new(Duration::ZERO, one);
    let inverted = Backoff::new(one * 2, one);

    // Assert
    assert!(zero_initial.is_err());
    assert!(inverted.is_err());
}
