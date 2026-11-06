//! A response body that sends `data` and then either ends cleanly, resets the stream or drops the connection.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use tokio::time::{Sleep, sleep};

use super::log::InFlightBlock;
use super::server::ConnControl;

const CHUNK: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Clean,
    Reset,
    DropConnection,
}

/// Declares `data.len()` bytes but sends only `stop` of them when the end is a fault. Before a fault ends
/// the response it waits `grace` so the bytes already sent reach the wire (a reset discards queued frames).
pub struct ScriptedBody {
    data: Bytes,
    pos: usize,
    stop: usize,
    end: End,
    grace: Duration,
    waiting: Option<Pin<Box<Sleep>>>,
    control: ConnControl,
    _in_flight: Option<InFlightBlock>,
}

impl ScriptedBody {
    pub fn new(
        data: Bytes,
        stop: usize,
        end: End,
        grace: Duration,
        control: ConnControl,
        in_flight: Option<InFlightBlock>,
    ) -> Self {
        let stop = stop.min(data.len());
        ScriptedBody {
            data,
            pos: 0,
            stop,
            end,
            grace,
            waiting: None,
            control,
            _in_flight: in_flight,
        }
    }
}

impl Body for ScriptedBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = self.get_mut();
        if this.pos < this.stop {
            let next = (this.pos + CHUNK).min(this.stop);
            let chunk = this.data.slice(this.pos..next);
            this.pos = next;
            return Poll::Ready(Some(Ok(Frame::data(chunk))));
        }
        if this.end == End::Clean {
            return Poll::Ready(None);
        }
        let grace = this.grace;
        let waiting = this.waiting.get_or_insert_with(|| Box::pin(sleep(grace)));
        if waiting.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        if this.end == End::DropConnection {
            this.control.abort();
        }
        Poll::Ready(Some(Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "scripted fault",
        ))))
    }

    fn is_end_stream(&self) -> bool {
        self.end == End::Clean && self.pos >= self.data.len()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact((self.data.len() - self.pos) as u64)
    }
}
