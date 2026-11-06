//! One scripted answer: status, headers, body, a delay and at most one transport fault.

use std::time::Duration;

use bytes::Bytes;
use sc_error::ErrorCode;
use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// A transport fault, counted in bytes of the response body (the part this response sends, so for a
/// ranged answer the count starts at the range start).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    None,
    /// Read the whole request, then close the connection without answering (a lost acknowledgement).
    DropConnection,
    /// Send headers and `n` body bytes, then close the connection.
    DropAfterBytes(usize),
    /// Read the whole request, then reset the stream before any response header.
    ResetStream,
    /// Send headers and `n` body bytes, then reset the stream. `n` = body length is "reset after the full body".
    ResetAfterBytes(usize),
}

/// The `application/problem+json` body of `problem_details.md`. `code` is free text so an unknown code can be scripted.
#[derive(Clone, Debug)]
pub struct Problem {
    pub code: String,
    pub status: u16,
    pub title: String,
    pub extras: Map<String, Value>,
}

#[derive(Clone, Debug)]
pub(super) enum Payload {
    Empty,
    Bytes {
        content_type: String,
        data: Bytes,
    },
    /// A resource served with `Range` support: 200, 206 or 416.
    Content(Bytes),
    Problem(Problem),
}

#[derive(Clone, Debug)]
pub struct Answer {
    pub(super) status: u16,
    pub(super) headers: Vec<(String, String)>,
    pub(super) payload: Payload,
    pub(super) delay: Duration,
    pub(super) fault: Fault,
    pub(super) flip: Option<(usize, u8)>,
}

impl Answer {
    fn new(status: u16, payload: Payload) -> Self {
        Answer {
            status,
            headers: Vec::new(),
            payload,
            delay: Duration::ZERO,
            fault: Fault::None,
            flip: None,
        }
    }

    pub fn status(status: u16) -> Self {
        Self::new(status, Payload::Empty)
    }

    pub fn no_content() -> Self {
        Self::status(204)
    }

    pub fn json<T: Serialize>(status: u16, value: &T) -> Self {
        let data = serde_json::to_vec(value).expect("scripted value serialises");
        Self::new(
            status,
            Payload::Bytes {
                content_type: "application/json".to_owned(),
                data: Bytes::from(data),
            },
        )
    }

    pub fn bytes(status: u16, content_type: &str, data: Bytes) -> Self {
        Self::new(
            status,
            Payload::Bytes {
                content_type: content_type.to_owned(),
                data,
            },
        )
    }

    /// A downloadable resource: the fixture honours one `Range` header (`content_serving_headers.md`).
    pub fn content(data: Bytes) -> Self {
        Self::new(200, Payload::Content(data))
    }

    /// A problem body for a code of the `sc_error` table, with that code's status and title.
    pub fn problem(code: ErrorCode) -> Self {
        Self::problem_with(code.as_str(), code.status(), code.title())
    }

    /// A problem body for any code and status, for codes the table does not know.
    pub fn problem_with(code: &str, status: u16, title: &str) -> Self {
        Self::new(
            status,
            Payload::Problem(Problem {
                code: code.to_owned(),
                status,
                title: title.to_owned(),
                extras: Map::new(),
            }),
        )
    }

    pub fn redirect(location: &str) -> Self {
        Self::status(302).header("Location", location)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Adds a field to the problem body (`detail`, `retry_after`, `need_blocks`, `reason`, `min_version`, `update_url`, ...).
    pub fn extra(mut self, name: &str, value: Value) -> Self {
        if let Payload::Problem(problem) = &mut self.payload {
            problem.extras.insert(name.to_owned(), value);
        }
        self
    }

    /// `retry_after` in the body and `Retry-After` in the headers, as `problem_details.md` requires.
    pub fn retry_after(self, seconds: u64) -> Self {
        self.extra("retry_after", json!(seconds))
            .header("Retry-After", &seconds.to_string())
    }

    pub fn need_blocks(self, blocks: &[u32]) -> Self {
        self.extra("need_blocks", json!(blocks))
    }

    /// Waits this long before answering.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn fault(mut self, fault: Fault) -> Self {
        self.fault = fault;
        self
    }

    /// Flips bit `bit` (0..8) of the byte at `offset` of the served resource.
    pub fn flip_bit(mut self, offset: usize, bit: u8) -> Self {
        self.flip = Some((offset, bit));
        self
    }
}

/// Request ids are UUIDv7-shaped and counted, so a rerun of the same script repeats them.
pub(super) fn request_id(seq: u64) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | u128::from(seq))
}
