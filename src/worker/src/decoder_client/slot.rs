use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use media_shared::decoder_protocol::Capabilities;
use sc_observability::{CheckFuture, ReadinessCheck, ReadinessItem, log_info, log_warn};
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

use super::conn::DecoderConn;
use super::error::{BackoffError, GateFailure};
use super::metrics::{DecoderMetrics, SlotName};

/// Reconnect delays: `initial`, doubling, never above `max`. Each delay is at least the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Result<Self, BackoffError> {
        if initial.is_zero() || initial > max {
            return Err(BackoffError);
        }
        Ok(Self { initial, max })
    }

    pub fn initial(&self) -> Duration {
        self.initial
    }

    pub fn after(&self, previous: Duration) -> Duration {
        previous.saturating_mul(2).min(self.max)
    }
}

pub struct SlotConfig {
    pub name: SlotName,
    /// `worker_sock_interactive` or `worker_sock_general`.
    pub socket: PathBuf,
    /// `host:port` targets the decoder must fail to reach (`self_check`).
    pub forbidden_targets: Vec<String>,
    /// `decoder_silence_timeout`: the longest the whole handshake (`hello` and `self_check`) may take.
    pub handshake_timeout: Duration,
    pub backoff: Backoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    /// No connection has passed the gate yet, or the last one ended.
    Connecting,
    /// The last connection was refused by the gate; the slot takes no jobs.
    GateFailed(GateFailure),
    Ready(Capabilities),
}

impl SlotState {
    pub fn is_ready(&self) -> bool {
        matches!(self, SlotState::Ready(_))
    }

    /// The `decoder_ok` of the status report.
    pub fn decoder_ok(&self) -> bool {
        self.is_ready()
    }

    /// The cached `hello` reply (build timestamp, NVENC fields) while the slot is ready.
    pub fn capabilities(&self) -> Option<&Capabilities> {
        match self {
            SlotState::Ready(capabilities) => Some(capabilities),
            SlotState::Connecting | SlotState::GateFailed(_) => None,
        }
    }
}

/// A connection that passed the gate: the only way to reach a decoder with a job.
pub struct GatedConn {
    pub conn: DecoderConn,
    pub capabilities: Capabilities,
}

/// One decoder slot: reconnects with backoff and gates every new connection. There is no background task;
/// the caller that owns the slot's job loop awaits [`Slot::next_ready`] whenever it has no connection.
pub struct Slot {
    config: SlotConfig,
    metrics: Arc<DecoderMetrics>,
    state: watch::Sender<SlotState>,
    handed_out: bool,
}

/// What one `next_ready` call has already reported, so an outage counts once.
struct Outage {
    restart_pending: bool,
    connect_failure_logged: bool,
}

impl Slot {
    pub fn new(config: SlotConfig, metrics: Arc<DecoderMetrics>) -> Self {
        metrics.selfcheck_ok.set(config.name, 0.0);
        Self {
            config,
            metrics,
            state: watch::channel(SlotState::Connecting).0,
            handed_out: false,
        }
    }

    pub fn name(&self) -> SlotName {
        self.config.name
    }

    pub fn subscribe(&self) -> watch::Receiver<SlotState> {
        self.state.subscribe()
    }

    /// The `/readyz` condition of this slot: ok only while the gate has passed on the live connection.
    pub fn readiness(&self) -> SlotReadiness {
        SlotReadiness {
            name: self.config.name,
            state: self.subscribe(),
        }
    }

    /// Connects, runs the gate and returns the connection once `hello` and an all-ok `self_check` passed;
    /// until then the slot is not ready. Call it again after the previous connection ended for any reason:
    /// that counts one `worker_decoder_restarts_total` however many attempts the outage takes. Dropping the
    /// future stops the attempts.
    pub async fn next_ready(&mut self) -> GatedConn {
        self.mark_not_ready(SlotState::Connecting);
        let mut outage = Outage {
            restart_pending: self.handed_out,
            connect_failure_logged: false,
        };
        let mut delay = self.config.backoff.initial();
        loop {
            if let Some(gated) = self.attempt(&mut outage).await {
                self.handed_out = true;
                self.state
                    .send_replace(SlotState::Ready(gated.capabilities.clone()));
                self.metrics.selfcheck_ok.set(self.config.name, 1.0);
                log_info!("decoder slot ready");
                return gated;
            }
            sleep(delay).await;
            delay = self.config.backoff.after(delay);
        }
    }

    fn mark_not_ready(&self, state: SlotState) {
        self.state.send_replace(state);
        self.metrics.selfcheck_ok.set(self.config.name, 0.0);
    }

    async fn attempt(&self, outage: &mut Outage) -> Option<GatedConn> {
        let mut conn = match DecoderConn::connect(&self.config.socket).await {
            Ok(conn) => conn,
            Err(error) => {
                if !mem::replace(&mut outage.connect_failure_logged, true) {
                    log_warn!("decoder socket not reachable", err = &error);
                }
                return None;
            }
        };
        if mem::take(&mut outage.restart_pending) {
            self.metrics.restarts.inc(self.config.name);
        }
        match timeout(self.config.handshake_timeout, self.gate(&mut conn)).await {
            Ok(Ok(capabilities)) => Some(GatedConn { conn, capabilities }),
            Ok(Err(failure)) => {
                self.mark_not_ready(SlotState::GateFailed(failure));
                None
            }
            Err(_) => {
                log_warn!("decoder handshake timed out");
                self.mark_not_ready(SlotState::GateFailed(GateFailure::Silent));
                None
            }
        }
    }

    async fn gate(&self, conn: &mut DecoderConn) -> Result<Capabilities, GateFailure> {
        let capabilities = conn.hello().await.map_err(|error| {
            log_warn!("decoder hello failed", err = &error);
            GateFailure::Hello
        })?;
        let report = conn
            .self_check(&self.config.forbidden_targets)
            .await
            .map_err(|error| {
                log_warn!("decoder self_check failed", err = &error);
                GateFailure::SelfCheck
            })?;
        let failed = report.0.iter().filter(|item| !item.ok).count();
        if report.0.is_empty() || failed > 0 {
            log_warn!("decoder self_check refused the slot");
            let failed = u32::try_from(failed.max(1)).unwrap_or(u32::MAX);
            return Err(GateFailure::Items { failed });
        }
        Ok(capabilities)
    }
}

/// `/readyz` answers 503 while this slot is not ready. The detail is fixed text, never decoder output.
pub struct SlotReadiness {
    name: SlotName,
    state: watch::Receiver<SlotState>,
}

impl ReadinessCheck for SlotReadiness {
    fn name(&self) -> &str {
        self.name.readiness_name()
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        let item = match &*self.state.borrow() {
            SlotState::Ready(_) => ReadinessItem::ok(),
            SlotState::Connecting => ReadinessItem::failing("connecting"),
            SlotState::GateFailed(failure) => ReadinessItem::failing(failure.detail()),
        };
        Box::pin(std::future::ready(item))
    }
}
