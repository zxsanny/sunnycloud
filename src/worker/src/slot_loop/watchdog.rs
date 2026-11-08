//! The watchdog around one decoder call (09 section 3, step 4, tier 2) and the lease abort.
//!
//! A call that runs past its wall limit, or goes silent for `decoder_silence_timeout`, or whose lease is lost,
//! is cancelled; the decoder then gets `decoder_cancel_grace` to answer before the call is dropped, which
//! closes the connection. Tier 1, the runner's own stall kill, answers `Stalled` inside the decoder and is not
//! seen here.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use media_shared::decoder_protocol::Event;
use sc_observability::log_warn;
use tokio::sync::Notify;
use tokio::time::{Instant, sleep_until, timeout};

use super::config::{KindConfig, LoopConfig};
use super::failure::Stop;
use super::lease::{Lease, LeaseEnd};
use crate::decoder_client::{CallError, Unhealthy};

/// The limits of one job's decoder calls.
#[derive(Debug, Clone, Copy)]
pub(super) struct Watchdog {
    silence: Duration,
    grace: Duration,
    wall: Duration,
}

/// The two signals a decoder call shares with its watcher: every event the decoder sends, and the request to
/// cancel the call.
pub(super) struct Tap {
    events: Arc<Notify>,
    cancel: Arc<Notify>,
}

enum Trip {
    Silent,
    WallLimit,
    Lease(LeaseEnd),
}

impl Tap {
    pub(super) fn new() -> Self {
        Self {
            events: Arc::new(Notify::new()),
            cancel: Arc::new(Notify::new()),
        }
    }

    /// The event sink of the call: any event proves the decoder is alive.
    pub(super) fn sink(&self) -> impl FnMut(Event) + Send + use<> {
        let events = Arc::clone(&self.events);
        move |_| events.notify_one()
    }

    /// The cancel future of the call; it completes once the watcher asks for a cancel.
    pub(super) fn cancel(&self) -> impl Future<Output = ()> + Send + use<> {
        let cancel = Arc::clone(&self.cancel);
        async move { cancel.notified().await }
    }
}

impl Watchdog {
    pub(super) fn new(config: &LoopConfig, kind: &KindConfig) -> Self {
        Self {
            silence: config.silence_timeout,
            grace: config.cancel_grace,
            wall: kind.wall(),
        }
    }

    /// Drives `call`, which must have been built with `tap`'s sink and cancel, to its answer. A typed answer
    /// of the decoder, including `Cancelled`, comes back as `Ok`; the cases that end the job early come back
    /// as the `Stop` that closes it. A slot-level stop (`Stop::Unhealthy`) leaves the connection for the
    /// caller to close.
    pub(super) async fn watch<T>(
        &self,
        lease: &mut Lease,
        tap: &Tap,
        call: impl Future<Output = Result<T, CallError>>,
    ) -> Result<Result<T, CallError>, Stop> {
        let mut call = std::pin::pin!(call);
        let started = Instant::now();
        let wall_at = started + self.wall;
        let mut silence_at = started + self.silence;
        let trip = loop {
            tokio::select! {
                biased;
                answer = &mut call => return Ok(answer),
                () = tap.events.notified() => silence_at = Instant::now() + self.silence,
                () = sleep_until(silence_at) => break Trip::Silent,
                () = sleep_until(wall_at) => break Trip::WallLimit,
                end = lease.ended() => break Trip::Lease(end),
            }
        };
        tap.cancel.notify_one();
        if timeout(self.grace, call.as_mut()).await.is_err() {
            log_warn!("decoder did not answer the cancel in time");
        }
        Err(match trip {
            Trip::Silent => Stop::Unhealthy(Unhealthy::Silent),
            Trip::WallLimit => Stop::Unhealthy(Unhealthy::WallLimit),
            Trip::Lease(end) => Stop::from(end),
        })
    }
}
