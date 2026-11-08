use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use hyper::body::{Body as HttpBody, Bytes, Frame, SizeHint};
use tokio::sync::Notify;
use tokio::time::{Instant, Sleep};

/// Why a request body was cut off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cut {
    /// More bytes than the class allows.
    TooLarge,
    /// The peer sent nothing for the idle timeout.
    Idle,
}

/// What a body tells the layer around the handler: set once, wakes the layer so it can end the request.
#[derive(Default)]
pub(super) struct Trip {
    cut: OnceLock<Cut>,
    wake: Notify,
}

impl Trip {
    fn set(&self, cut: Cut) {
        if self.cut.set(cut).is_ok() {
            self.wake.notify_one();
        }
    }

    pub(super) fn cut(&self) -> Option<Cut> {
        self.cut.get().copied()
    }

    pub(super) async fn tripped(&self) -> Cut {
        loop {
            if let Some(cut) = self.cut() {
                return cut;
            }
            self.wake.notified().await;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum BodyError {
    #[error("request body is over the limit of its class")]
    TooLarge,
    #[error("request body was idle for too long")]
    Idle,
    #[error("request body could not be read")]
    Read(#[source] axum::Error),
    #[error("request ended by a time limit")]
    Aborted,
}

struct IdleTimer {
    limit: Duration,
    sleep: Pin<Box<Sleep>>,
    armed: bool,
}

/// A request body that streams through to the handler, counts its bytes against `remaining` and, when an idle
/// limit is set, fails if the peer sends nothing for that long while the handler waits for data. Nothing is
/// buffered.
pub(super) struct GuardedBody {
    inner: Body,
    remaining: u64,
    idle: Option<IdleTimer>,
    trip: Arc<Trip>,
    failed: Option<Cut>,
}

impl GuardedBody {
    pub(super) fn new(
        inner: Body,
        limit: u64,
        idle: Option<Duration>,
        trip: Arc<Trip>,
    ) -> GuardedBody {
        GuardedBody {
            inner,
            remaining: limit,
            idle: idle.map(|limit| IdleTimer {
                limit,
                sleep: Box::pin(tokio::time::sleep(limit)),
                armed: false,
            }),
            trip,
            failed: None,
        }
    }

    fn fail(&mut self, cut: Cut) -> Poll<Option<Result<Frame<Bytes>, BodyError>>> {
        self.failed = Some(cut);
        self.trip.set(cut);
        Poll::Ready(Some(Err(error_of(cut))))
    }
}

fn error_of(cut: Cut) -> BodyError {
    match cut {
        Cut::TooLarge => BodyError::TooLarge,
        Cut::Idle => BodyError::Idle,
    }
}

impl HttpBody for GuardedBody {
    type Data = Bytes;
    type Error = BodyError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BodyError>>> {
        let this = self.get_mut();
        if let Some(cut) = this.failed {
            return Poll::Ready(Some(Err(error_of(cut))));
        }
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(idle) = this.idle.as_mut() {
                    idle.armed = false;
                }
                let sent = frame
                    .data_ref()
                    .map_or(0, |data| u64::try_from(data.len()).unwrap_or(u64::MAX));
                if sent > this.remaining {
                    return this.fail(Cut::TooLarge);
                }
                this.remaining -= sent;
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(BodyError::Read(error)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                let Some(idle) = this.idle.as_mut() else {
                    return Poll::Pending;
                };
                if !idle.armed {
                    idle.sleep.as_mut().reset(Instant::now() + idle.limit);
                    idle.armed = true;
                }
                if idle.sleep.as_mut().poll(cx).is_ready() {
                    return this.fail(Cut::Idle);
                }
                Poll::Pending
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.failed.is_none() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

/// A response body that fails on its first poll, so hyper resets the stream (HTTP/2) or closes the connection
/// (HTTP/1.1) instead of ending it cleanly.
pub(super) struct AbortedBody;

impl HttpBody for AbortedBody {
    type Data = Bytes;
    type Error = BodyError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BodyError>>> {
        Poll::Ready(Some(Err(BodyError::Aborted)))
    }
}
