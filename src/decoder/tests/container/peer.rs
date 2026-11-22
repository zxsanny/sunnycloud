//! The protocol client of the harness, built on `media-shared::decoder_protocol` through the frame
//! reader of the shared test support.

use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{Capabilities, Event, Limits, Reply, Request, Response};

use crate::common::{Client, Received};
use crate::decoder::Job;

pub struct Peer {
    client: Client,
    /// `Alive` events seen on this connection.
    pub alive: u32,
}

impl Peer {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            client: Client::new(stream),
            alive: 0,
        }
    }

    pub fn send(&mut self, request: Request) {
        self.client.send(request);
    }

    /// Bytes written exactly as given, so a case can break the framing.
    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.client.send_raw(bytes);
    }

    pub fn send_payload(&mut self, payload: &[u8]) {
        self.client.send_payload(payload);
    }

    /// The next response within `within`, skipping events; `None` when the connection closed or
    /// nothing arrived.
    pub fn try_response(&mut self, within: Duration) -> Option<Response> {
        match self.next_response(within) {
            Received::Reply(Reply::Response(response)) => Some(response),
            _ => None,
        }
    }

    /// Like [`Peer::try_response`] but says what happened instead of `None`.
    pub fn next_response(&mut self, within: Duration) -> Received {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.client.recv(left) {
                Received::Reply(Reply::Event(Event::Alive)) => self.alive += 1,
                Received::Reply(Reply::Event(Event::Progress { .. })) => {}
                other => return other,
            }
        }
    }

    pub fn response(&mut self, within: Duration) -> Response {
        match self.next_response(within) {
            Received::Reply(Reply::Response(response)) => response,
            other => panic!("expected a response within {within:?}, got {other:?}"),
        }
    }

    pub fn call(&mut self, request: Request, within: Duration) -> Response {
        self.send(request);
        self.response(within)
    }

    pub fn hello(&mut self) -> Capabilities {
        match self.call(Request::Hello { protocol: 1 }, Duration::from_secs(5)) {
            Response::Capabilities(capabilities) => capabilities,
            other => panic!("hello answered {other:?}"),
        }
    }
}

/// The limits a supervisor sends for a photo job: the documented ceilings.
pub fn photo_limits() -> Limits {
    Limits {
        cpu: 2,
        memory_bytes: 2 << 30,
        wall: 120_000,
        stall_timeout: 90_000,
        max_output_bytes: 32 << 20,
    }
}

/// A request for more than any ceiling allows (IT-10-3, IT-10-12).
pub fn greedy_limits() -> Limits {
    Limits {
        cpu: 64,
        memory_bytes: 64 << 30,
        wall: 3_600_000,
        stall_timeout: 3_600_000,
        max_output_bytes: 1 << 40,
    }
}

pub fn render(job: &Job, max_edge: u32, limits: Limits) -> Request {
    Request::RenderStill {
        job_dir: job.dir(),
        max_edge,
        poster_at_ms: None,
        limits,
    }
}

pub fn render_at(job: &Job, max_edge: u32, poster_at_ms: u64) -> Request {
    Request::RenderStill {
        job_dir: job.dir(),
        max_edge,
        poster_at_ms: Some(poster_at_ms),
        limits: photo_limits(),
    }
}

pub fn probe(job: &Job) -> Request {
    Request::Probe { job_dir: job.dir() }
}
