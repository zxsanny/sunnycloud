//! The frame codec: a 4-byte big-endian unsigned length, then that many bytes of UTF-8 JSON.
//! Synchronous over `std::io`; no I/O of its own.

use std::fmt;
use std::io::{self, ErrorKind, Read, Write};

use crate::decoder_protocol::message::{encode_message, parse_reply, parse_request};
use crate::decoder_protocol::{Message, ProtocolViolation, Reply, Request, ViolationKind};

/// Largest payload of one frame (64 KiB).
pub const MAX_PAYLOAD: usize = 65_536;
pub const PREFIX_LEN: usize = 4;

#[derive(Debug)]
pub enum ReadError {
    Violation(ProtocolViolation),
    Io(io::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Violation(violation) => violation.fmt(f),
            ReadError::Io(error) => write!(f, "decoder socket read failed: {error}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<ProtocolViolation> for ReadError {
    fn from(violation: ProtocolViolation) -> Self {
        ReadError::Violation(violation)
    }
}

#[derive(Debug)]
pub enum WriteError {
    /// The message is out of range or too large: a bug of the sender, nothing was written.
    Invalid(ProtocolViolation),
    Io(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Invalid(violation) => write!(f, "refused to send: {violation}"),
            WriteError::Io(error) => write!(f, "decoder socket write failed: {error}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Result of one poll of a [`FrameReader`].
#[derive(Debug, PartialEq, Eq)]
pub enum Polled<T> {
    Ready(T),
    /// The source has no more bytes for now (`WouldBlock` or `TimedOut`); poll again later.
    Pending,
    /// The peer closed the stream, at a frame boundary or inside a frame. A partial frame is
    /// dropped and never becomes a message. Not a violation.
    Closed,
}

/// Incremental reader of frames. It keeps the bytes of at most one incomplete frame and never
/// reads past the end of that frame. Dropping it frees the partial buffer.
#[derive(Debug, Default)]
pub struct FrameReader {
    prefix: [u8; PREFIX_LEN],
    prefix_filled: usize,
    /// Allocated once, after the announced length passed the cap, with exactly that length.
    payload: Vec<u8>,
    payload_filled: usize,
    announced: Option<usize>,
    failed: Option<ProtocolViolation>,
}

enum Fill {
    Full,
    Pending,
    Closed,
}

fn fill<R: Read>(source: &mut R, buf: &mut [u8], filled: &mut usize) -> io::Result<Fill> {
    while *filled < buf.len() {
        match source.read(&mut buf[*filled..]) {
            Ok(0) => return Ok(Fill::Closed),
            Ok(read) => *filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Ok(Fill::Pending);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Fill::Full)
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Capacity of the payload buffer; never above [`MAX_PAYLOAD`].
    pub fn buffered_capacity(&self) -> usize {
        self.payload.capacity()
    }

    fn reset(&mut self) {
        *self = Self {
            failed: self.failed,
            ..Self::default()
        };
    }

    fn fail(&mut self, kind: ViolationKind) -> ReadError {
        let violation = ProtocolViolation(kind);
        self.failed = Some(violation);
        self.payload = Vec::new();
        ReadError::Violation(violation)
    }

    /// Reads what the source has and returns the payload of one complete frame. After a
    /// violation every later poll returns the same violation: the caller drops the connection.
    pub fn poll_payload<R: Read>(&mut self, source: &mut R) -> Result<Polled<Vec<u8>>, ReadError> {
        if let Some(violation) = self.failed {
            return Err(violation.into());
        }
        if self.announced.is_none() {
            match fill(source, &mut self.prefix, &mut self.prefix_filled).map_err(ReadError::Io)? {
                Fill::Full => {}
                Fill::Pending => return Ok(Polled::Pending),
                Fill::Closed => {
                    self.reset();
                    return Ok(Polled::Closed);
                }
            }
            let announced = u32::from_be_bytes(self.prefix) as usize;
            if announced > MAX_PAYLOAD {
                return Err(self.fail(ViolationKind::Oversize));
            }
            self.payload = vec![0; announced];
            self.announced = Some(announced);
        }
        match fill(source, &mut self.payload, &mut self.payload_filled).map_err(ReadError::Io)? {
            Fill::Full => {
                let payload = std::mem::take(&mut self.payload);
                self.reset();
                Ok(Polled::Ready(payload))
            }
            Fill::Pending => Ok(Polled::Pending),
            Fill::Closed => {
                self.reset();
                Ok(Polled::Closed)
            }
        }
    }

    /// Runner side: the next request. A violation of the parser is returned as `ReadError`.
    pub fn poll_request<R: Read>(&mut self, source: &mut R) -> Result<Polled<Request>, ReadError> {
        match self.poll_payload(source)? {
            Polled::Ready(payload) => parse_request(&payload)
                .map(Polled::Ready)
                .map_err(|violation| self.fail_with(violation)),
            Polled::Pending => Ok(Polled::Pending),
            Polled::Closed => Ok(Polled::Closed),
        }
    }

    /// Supervisor side: the next response or event.
    pub fn poll_reply<R: Read>(&mut self, source: &mut R) -> Result<Polled<Reply>, ReadError> {
        match self.poll_payload(source)? {
            Polled::Ready(payload) => parse_reply(&payload)
                .map(Polled::Ready)
                .map_err(|violation| self.fail_with(violation)),
            Polled::Pending => Ok(Polled::Pending),
            Polled::Closed => Ok(Polled::Closed),
        }
    }

    fn fail_with(&mut self, violation: ProtocolViolation) -> ReadError {
        self.fail(violation.0)
    }
}

/// Prefix and payload in one buffer. The cap holds for the sender too.
fn frame_bytes(payload: &[u8]) -> Result<Vec<u8>, ProtocolViolation> {
    let oversize = ProtocolViolation(ViolationKind::Oversize);
    if payload.len() > MAX_PAYLOAD {
        return Err(oversize);
    }
    let length = u32::try_from(payload.len()).map_err(|_| oversize)?;
    let mut frame = Vec::with_capacity(PREFIX_LEN + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Frames and writes one message with a single `write_all`. Nothing is written for a message
/// that the parser would refuse.
pub fn write_message<W: Write>(sink: &mut W, message: &Message) -> Result<(), WriteError> {
    let payload = encode_message(message).map_err(WriteError::Invalid)?;
    let frame = frame_bytes(&payload).map_err(WriteError::Invalid)?;
    sink.write_all(&frame).map_err(WriteError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_above_the_cap_is_not_framed() {
        let result = frame_bytes(&vec![b' '; MAX_PAYLOAD + 1]);

        assert_eq!(result, Err(ProtocolViolation(ViolationKind::Oversize)));
    }

    #[test]
    fn a_payload_at_the_cap_is_framed_with_a_big_endian_prefix() {
        let frame = frame_bytes(&vec![b' '; MAX_PAYLOAD]).expect("frame");

        assert_eq!(frame[..PREFIX_LEN], [0, 1, 0, 0]);
        assert_eq!(frame.len(), PREFIX_LEN + MAX_PAYLOAD);
    }
}
