use std::collections::BTreeMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;

use sc_config::WorkerSettings;
use sc_observability::logging::Level;
use sc_observability::{
    BuildInfo, Component, Liveness, LogConfig, LogFormat, Metrics, ObsError, ReadinessRegistry,
    init_logging, log_info, ops_router,
};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::error::WorkerError;
use crate::probe::{self, probe_target};
use crate::settings::{load_settings, ops_addr};

const READINESS_PATH: &str = "/readyz";

/// The process: logging, the metrics registry and the ops listener, until SIGTERM or SIGINT.
///
/// The readiness checks of the decoder gate and the slots are added to `readiness` by their tasks; with none
/// registered `/readyz` answers 200.
pub async fn serve(env: BTreeMap<String, String>) -> Result<(), WorkerError> {
    let settings = load_settings(env)?;
    let addr = ops_addr(&settings)?;
    let _log = init_logging(log_config(&settings)?)?;
    let metrics = Arc::new(Metrics::new(BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("SUNNYCLOUD_GIT_COMMIT").unwrap_or("unknown"),
        test_clock: cfg!(feature = "test-clock"),
    })?);
    let readiness = ReadinessRegistry::new();
    let liveness = Liveness::new();
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| WorkerError::io("cannot bind the ops listener", source))?;
    log_info!("worker started");
    serve_ops(
        listener,
        ops_router(metrics, readiness, liveness.clone()),
        async move {
            termination().await;
            liveness.shut_down();
        },
    )
    .await
}

/// Serves `router` on `listener` until `shutdown` completes.
pub async fn serve_ops(
    listener: TcpListener,
    router: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), WorkerError> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|source| WorkerError::io("ops listener failed", source))
}

/// `sunnycloud-worker healthcheck`: `/readyz` on the configured ops address answers 200.
pub async fn healthcheck(env: BTreeMap<String, String>) -> Result<(), WorkerError> {
    let settings = load_settings(env)?;
    let target = probe_target(ops_addr(&settings)?);
    probe::get_ok(target, READINESS_PATH).await?;
    Ok(())
}

fn log_config(settings: &WorkerSettings) -> Result<LogConfig, ObsError> {
    let level = Level::from_str(&settings.log_level)
        .map_err(|_| ObsError::BadConfig("log_level is not a level".to_owned()))?;
    Ok(LogConfig {
        format: LogFormat::from_str(&settings.log)?,
        level,
        component: Component::Worker,
    })
}

async fn termination() {
    match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(mut term), Ok(mut int)) => tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        },
        (Err(error), _) | (_, Err(error)) => {
            // Without signal handlers the worker could not be stopped cleanly; stopping now is the safe side.
            eprintln!("error: cannot install the signal handlers: {error}");
        }
    }
}
