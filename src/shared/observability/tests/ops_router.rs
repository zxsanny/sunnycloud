//! AC-4 readiness semantics, AC-5 liveness is shallow, AC-8 (the part this crate owns: serving the ops
//! routes logs nothing at info), `/metrics` content type.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::config;
use sc_observability::{
    BuildInfo, CheckFuture, Liveness, LogFormat, Metrics, ReadinessCheck, ReadinessItem,
    ReadinessRegistry, ReadinessStatus, ops_router,
};
use serde_json::Value;
use tower::ServiceExt;
use tracing::Level;

struct Switch {
    name: &'static str,
    ok: AtomicBool,
}

impl Switch {
    fn new(name: &'static str, ok: bool) -> Arc<Switch> {
        Arc::new(Switch {
            name,
            ok: AtomicBool::new(ok),
        })
    }
}

impl ReadinessCheck for Switch {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async move {
            if self.ok.load(Ordering::SeqCst) {
                ReadinessItem::ok()
            } else {
                ReadinessItem::failing("down for the test")
            }
        })
    }
}

struct Hangs;

impl ReadinessCheck for Hangs {
    fn name(&self) -> &str {
        "hangs"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            ReadinessItem::ok()
        })
    }
}

fn metrics() -> Arc<Metrics> {
    Arc::new(
        Metrics::new(BuildInfo {
            version: "0.1.0",
            commit: "abc1234",
            test_clock: false,
        })
        .expect("metrics"),
    )
}

async fn get(router: &Router, path: &str) -> (StatusCode, String, Option<String>) {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().expect("ascii").to_owned());
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("utf8"),
        content_type,
    )
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("json body")
}

#[tokio::test]
async fn ac4_readyz_is_503_naming_the_failing_check_then_200_then_503_on_shutdown() {
    let (database, blobs, scheduler) = (
        Switch::new("database", true),
        Switch::new("blob_volume", false),
        Switch::new("scheduler", true),
    );
    let mut registry = ReadinessRegistry::new();
    registry.add(database.clone());
    registry.add(blobs.clone());
    registry.add(scheduler);
    let liveness = Liveness::new();
    let router = ops_router(metrics(), registry, liveness.clone());

    let (status, body, content_type) = get(&router, "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    let body = json(&body);
    assert_eq!(body["status"], "degraded");
    let failing: Vec<_> = body["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter(|check| check["status"] != "ok")
        .collect();
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0]["name"], "blob_volume");
    assert_eq!(failing[0]["status"], "failing");
    assert_eq!(failing[0]["detail"], "down for the test");
    assert_eq!(body["checks"].as_array().expect("checks").len(), 3);

    blobs.ok.store(true, Ordering::SeqCst);
    let (status, body, _) = get(&router, "/readyz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["status"], "ok");

    liveness.shut_down();
    let (status, body, _) = get(&router, "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["checks"][0]["name"], "shutdown");
}

#[tokio::test]
async fn ac4_a_degraded_check_also_turns_readyz_to_503() {
    struct Degraded;
    impl ReadinessCheck for Degraded {
        fn name(&self) -> &str {
            "blobs"
        }
        fn check(&self) -> CheckFuture<'_, ReadinessItem> {
            Box::pin(async { ReadinessItem::new(ReadinessStatus::Degraded, "1 suspect blob") })
        }
    }
    let mut registry = ReadinessRegistry::new();
    registry.add(Arc::new(Degraded));
    let router = ops_router(metrics(), registry, Liveness::new());

    let (status, body, _) = get(&router, "/readyz").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(&body)["checks"][0]["status"], "degraded");
}

#[tokio::test]
async fn ac4_a_check_that_hangs_fails_after_its_timeout_and_does_not_block_the_others() {
    let mut registry = ReadinessRegistry::new().with_timeout(Duration::from_millis(50));
    registry.add(Switch::new("database", true));
    registry.add(Arc::new(Hangs));
    let router = ops_router(metrics(), registry, Liveness::new());

    let started = std::time::Instant::now();
    let (status, body, _) = get(&router, "/readyz").await;

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let body = json(&body);
    assert_eq!(body["checks"][0]["status"], "ok");
    assert_eq!(body["checks"][1]["name"], "hangs");
    assert_eq!(body["checks"][1]["detail"], "check timed out");
}

#[tokio::test]
async fn readyz_with_no_registered_check_is_ok() {
    let router = ops_router(metrics(), ReadinessRegistry::new(), Liveness::new());

    let (status, body, _) = get(&router, "/readyz").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["checks"].as_array().expect("checks").len(), 0);
}

#[tokio::test]
async fn ac5_healthz_answers_200_with_a_failing_check_registered_and_after_shutdown() {
    let mut registry = ReadinessRegistry::new();
    registry.add(Switch::new("database", false));
    let liveness = Liveness::new();
    let router = ops_router(metrics(), registry, liveness.clone());

    let (status, body, _) = get(&router, "/healthz").await;
    assert_eq!(
        (status, json(&body)["status"].clone()),
        (StatusCode::OK, "ok".into())
    );

    liveness.shut_down();
    let (status, _, _) = get(&router, "/healthz").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn metrics_route_serves_the_prometheus_text_content_type() {
    let router = ops_router(metrics(), ReadinessRegistry::new(), Liveness::new());

    let (status, body, content_type) = get(&router, "/metrics").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/plain; version=0.0.4"));
    assert!(body.contains("sunnycloud_build_info{"));
}

#[tokio::test]
async fn ac8_serving_a_thousand_successful_ops_requests_logs_no_info_line() {
    let mut registry = ReadinessRegistry::new();
    registry.add(Switch::new("database", true));
    let router = ops_router(metrics(), registry, Liveness::new());

    let (sink, statuses) = {
        let sink = common::Capture::default();
        let clock = Arc::new(sc_clock::ManualClock::new(common::START));
        let (subscriber, _) = sc_observability::subscriber(
            config(LogFormat::Json, Level::INFO),
            clock,
            Arc::new(sink.clone()),
        );
        let _guard = tracing::subscriber::set_default(subscriber);
        let mut statuses = Vec::new();
        for index in 0..1000 {
            let path = ["/healthz", "/readyz", "/metrics"][index % 3];
            statuses.push(get(&router, path).await.0);
        }
        (sink, statuses)
    };

    assert!(statuses.iter().all(|status| *status == StatusCode::OK));
    assert!(sink.lines().is_empty(), "{:?}", sink.lines());
}
