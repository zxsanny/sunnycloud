use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use sc_clock::Clock;
use sc_config::WorkerSettings;
use sc_observability::logging::Level;
use sc_observability::{
    BuildInfo, Component, Liveness, LogConfig, LogFormat, Metrics, ObsError, init_logging,
    log_error, log_info, log_warn, ops_router,
};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::clock_source::ProcessClock;
use crate::compose::{assemble, worker_id};
use crate::error::WorkerError;
use crate::probe::{self, probe_target};
use crate::scratch::sweep_all;
use crate::settings::{load_settings, ops_addr};
use crate::slot_loop::Shutdown;

const READINESS_PATH: &str = "/readyz";

/// The process: logging, the metrics registry, then the supervisor until SIGTERM or SIGINT.
pub async fn serve(env: BTreeMap<String, String>) -> Result<(), WorkerError> {
    let settings = load_settings(env)?;
    let _log = init_logging(log_config(&settings)?)?;
    let metrics = Arc::new(Metrics::new(BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("SUNNYCLOUD_GIT_COMMIT").unwrap_or("unknown"),
        test_clock: cfg!(feature = "test-clock"),
    })?);
    let worker_id = worker_id(&settings);
    let clock = ProcessClock::start(&settings, &worker_id)?;
    run_worker(settings, worker_id, metrics, clock.clock(), termination()).await
}

/// The supervisor (09 section 3): sweep both scratch roots, serve the ops endpoints, run both slot loops and
/// the status reporter, and on `stop` drain.
///
/// No slot claims before the sweep is done and its own decoder has passed the gate. On `stop` the slots stop
/// claiming and `/readyz` answers 503; running jobs get `worker_shutdown_grace` to finish, then they are
/// cancelled and their leases released. Returns once every slot loop has ended.
pub async fn run_worker(
    settings: Arc<WorkerSettings>,
    worker_id: String,
    metrics: Arc<Metrics>,
    clock: Arc<dyn Clock>,
    stop: impl Future<Output = ()>,
) -> Result<(), WorkerError> {
    let addr = ops_addr(&settings)?;
    let shutdown = Shutdown::new();
    let parts = assemble(&settings, &worker_id, &metrics, clock, &shutdown)?;
    sweep_all(&parts.roots)?;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| WorkerError::io("cannot bind the ops listener", source))?;
    let liveness = Liveness::new();
    let (ops_stop, ops_stopped) = oneshot::channel::<()>();
    let router = ops_router(metrics, parts.readiness, liveness.clone());
    let mut ops = tokio::spawn(serve_ops(listener, router, async move {
        // A dropped sender also ends the listener: the supervisor is gone.
        if let Err(error) = ops_stopped.await {
            log_warn!("ops listener stops: the supervisor is gone", err = &error);
        }
    }));
    let reporter = tokio::spawn(parts.status.run());
    let mut slots = JoinSet::new();
    for mut slot in parts.slots {
        slots.spawn(async move { slot.run().await });
    }
    log_info!("worker started");
    let ops_result = tokio::select! {
        () = stop => None,
        ended = &mut ops => Some(ended),
    };
    liveness.shut_down();
    drain(&mut slots, &shutdown, settings.worker_shutdown_grace.get()).await;
    reporter.abort();
    let ops_result = match ops_result {
        Some(ended) => ended,
        None => {
            if ops_stop.send(()).is_err() {
                log_warn!("ops listener had already ended");
            }
            ops.await
        }
    };
    ops_result
        .map_err(|source| WorkerError::io("ops listener task failed", io::Error::other(source)))?
}

/// Stops claiming, waits up to `grace` for the running jobs, then cancels and releases what is left.
async fn drain(slots: &mut JoinSet<()>, shutdown: &Shutdown, grace: Duration) {
    log_info!("shutdown started: no more claims");
    shutdown.drain();
    if timeout(grace, join_all(slots)).await.is_err() {
        log_warn!("shutdown grace passed: cancelling running jobs");
        shutdown.abort();
        join_all(slots).await;
    }
}

async fn join_all(slots: &mut JoinSet<()>) {
    while let Some(ended) = slots.join_next().await {
        if let Err(error) = ended {
            log_error!("a slot loop ended abnormally", err = &error);
        }
    }
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
