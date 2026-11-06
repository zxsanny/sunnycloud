//! The ops router shared by the API and the worker: `/healthz`, `/readyz`, `/metrics`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::future::join_all;
use serde::Serialize;

use crate::health::{ReadinessCheck, ReadinessItem, ReadinessStatus};
use crate::metrics::Metrics;

/// Longest a single readiness check may take before it counts as failing.
pub const DEFAULT_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// The checks `/readyz` runs. Built at startup, read-only once the router owns it.
#[derive(Clone)]
pub struct ReadinessRegistry {
    checks: Vec<Arc<dyn ReadinessCheck>>,
    timeout: Duration,
}

impl Default for ReadinessRegistry {
    fn default() -> Self {
        ReadinessRegistry {
            checks: Vec::new(),
            timeout: DEFAULT_CHECK_TIMEOUT,
        }
    }
}

impl ReadinessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the timeout each check gets (default [`DEFAULT_CHECK_TIMEOUT`]).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn add(&mut self, check: Arc<dyn ReadinessCheck>) {
        self.checks.push(check);
    }

    async fn run(&self) -> Vec<CheckReport> {
        let timeout = self.timeout;
        join_all(self.checks.iter().map(|check| async move {
            let item = tokio::time::timeout(timeout, check.check())
                .await
                .unwrap_or_else(|_| ReadinessItem::failing("check timed out"));
            CheckReport {
                name: check.name().to_owned(),
                item,
            }
        }))
        .await
    }
}

/// Process-wide switch behind `/readyz`. Clones share the flag; `/healthz` ignores it.
#[derive(Clone, Debug, Default)]
pub struct Liveness {
    shutting_down: Arc<AtomicBool>,
}

impl Liveness {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called on the shutdown signal: `/readyz` answers 503 from now on, without running the checks.
    pub fn shut_down(&self) {
        self.shutting_down.store(true, Ordering::Release);
    }

    pub fn is_shut_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }
}

#[derive(Serialize)]
struct CheckReport {
    name: String,
    #[serde(flatten)]
    item: ReadinessItem,
}

#[derive(Serialize)]
struct ReadyBody {
    status: &'static str,
    checks: Vec<CheckReport>,
}

#[derive(Clone)]
struct OpsState {
    metrics: Arc<Metrics>,
    readiness: Arc<ReadinessRegistry>,
    liveness: Liveness,
}

/// `/healthz` checks nothing, `/readyz` runs every registered check (503 when one is not ok or after
/// [`Liveness::shut_down`]), `/metrics` renders the registry in the Prometheus text format.
pub fn ops_router(
    metrics: Arc<Metrics>,
    readiness: ReadinessRegistry,
    liveness: Liveness,
) -> Router {
    let state = OpsState {
        metrics,
        readiness: Arc::new(readiness),
        liveness,
    };
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(scrape))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    json_response(StatusCode::OK, &serde_json::json!({ "status": "ok" }))
}

async fn readyz(State(state): State<OpsState>) -> Response {
    let checks = if state.liveness.is_shut_down() {
        vec![CheckReport {
            name: "shutdown".to_owned(),
            item: ReadinessItem::failing("shutting down"),
        }]
    } else {
        state.readiness.run().await
    };
    let all_ok = checks
        .iter()
        .all(|report| report.item.status == ReadinessStatus::Ok);
    let body = ReadyBody {
        status: if all_ok { "ok" } else { "degraded" },
        checks,
    };
    let code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    json_response(code, &body)
}

async fn scrape(State(state): State<OpsState>) -> Response {
    match state.metrics.render().await {
        Ok(text) => ([(header::CONTENT_TYPE, prometheus::TEXT_FORMAT)], text).into_response(),
        Err(error) => {
            crate::log_error!("metrics render failed", err = &error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn json_response<T: Serialize>(code: StatusCode, body: &T) -> Response {
    match serde_json::to_vec(body) {
        Ok(bytes) => (code, [(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(error) => {
            crate::log_error!("ops body serialization failed", err = &error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
