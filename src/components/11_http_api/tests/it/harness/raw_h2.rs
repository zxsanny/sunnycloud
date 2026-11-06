//! A minimal HTTP/2 client that writes and reads frames itself, so a test sees exactly what the server put
//! on the wire (SETTINGS, WINDOW_UPDATE, PING, GOAWAY) and can send what a library client refuses to.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub mod frame_type {
    pub const HEADERS: u8 = 0x1;
    pub const RST_STREAM: u8 = 0x3;
    pub const SETTINGS: u8 = 0x4;
    pub const PING: u8 = 0x6;
    pub const GOAWAY: u8 = 0x7;
    pub const WINDOW_UPDATE: u8 = 0x8;
    pub const CONTINUATION: u8 = 0x9;
}

pub mod setting {
    pub const MAX_CONCURRENT_STREAMS: u16 = 0x3;
    pub const INITIAL_WINDOW_SIZE: u16 = 0x4;
    pub const MAX_HEADER_LIST_SIZE: u16 = 0x6;
}

pub const END_STREAM: u8 = 0x1;
pub const END_HEADERS: u8 = 0x4;
pub const ACK: u8 = 0x1;

/// The window every HTTP/2 connection starts with, before any WINDOW_UPDATE.
pub const DEFAULT_WINDOW: u32 = 65_535;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub flags: u8,
    pub stream: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum Read {
    Frame(Frame),
    /// The server closed the connection (end of stream or reset).
    Closed,
    /// Nothing arrived within the wait.
    Quiet,
}

pub struct RawH2 {
    stream: TcpStream,
}

impl RawH2 {
    /// Connects, sends the connection preface and an empty SETTINGS frame.
    pub async fn connect(addr: SocketAddr) -> RawH2 {
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        let mut raw = RawH2 { stream };
        let mut hello = PREFACE.to_vec();
        hello.extend(encode_frame(frame_type::SETTINGS, 0, 0, &[]));
        raw.write(&hello)
            .await
            .expect("the server accepts the preface");
        raw
    }

    /// Fails once the server has closed the connection.
    pub async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes).await?;
        self.stream.flush().await
    }

    pub async fn send_frame(
        &mut self,
        kind: u8,
        flags: u8,
        stream: u32,
        payload: &[u8],
    ) -> io::Result<()> {
        self.write(&encode_frame(kind, flags, stream, payload))
            .await
    }

    /// The next frame, waiting at most `wait`.
    pub async fn read(&mut self, wait: Duration) -> Read {
        match tokio::time::timeout(wait, self.read_frame()).await {
            Err(_) => Read::Quiet,
            Ok(Ok(frame)) => Read::Frame(frame),
            Ok(Err(_)) => Read::Closed,
        }
    }

    async fn read_frame(&mut self) -> io::Result<Frame> {
        let mut head = [0u8; 9];
        self.stream.read_exact(&mut head).await?;
        let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
        let mut payload = vec![0u8; length];
        self.stream.read_exact(&mut payload).await?;
        Ok(Frame {
            kind: head[3],
            flags: head[4],
            stream: u32::from_be_bytes([head[5], head[6], head[7], head[8]]) & 0x7fff_ffff,
            payload,
        })
    }
}

pub fn encode_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).expect("frame payload fits 24 bits");
    let mut out = Vec::with_capacity(9 + payload.len());
    out.extend_from_slice(&length.to_be_bytes()[1..]);
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The (identifier, value) pairs of a SETTINGS frame.
pub fn settings(frame: &Frame) -> Vec<(u16, u32)> {
    frame
        .payload
        .chunks_exact(6)
        .map(|entry| {
            (
                u16::from_be_bytes([entry[0], entry[1]]),
                u32::from_be_bytes([entry[2], entry[3], entry[4], entry[5]]),
            )
        })
        .collect()
}

pub fn window_increment(frame: &Frame) -> u32 {
    u32::from_be_bytes([
        frame.payload[0],
        frame.payload[1],
        frame.payload[2],
        frame.payload[3],
    ]) & 0x7fff_ffff
}

/// HPACK for `GET / http://localhost` plus any `extra` literal headers (never indexed, no Huffman).
pub fn request_block(extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut block = vec![0x82, 0x86, 0x84]; // :method GET, :scheme http, :path /
    block.push(0x41); // :authority, literal with incremental indexing, name index 1
    block.push(9);
    block.extend_from_slice(b"localhost");
    for (name, value) in extra {
        block.push(0x00);
        push_string(&mut block, name.as_bytes());
        push_string(&mut block, value);
    }
    block
}

fn push_string(block: &mut Vec<u8>, text: &[u8]) {
    let length = text.len();
    if length < 127 {
        block.push(u8::try_from(length).expect("below 127"));
    } else {
        block.push(0x7f);
        let mut rest = length - 127;
        while rest >= 128 {
            block.push(u8::try_from(rest % 128).expect("below 128") | 0x80);
            rest /= 128;
        }
        block.push(u8::try_from(rest).expect("below 128"));
    }
    block.extend_from_slice(text);
}

/// `true` when a response header block starts with an indexed `:status` of 200, 204 or 206.
pub fn is_success_status(block: &[u8]) -> bool {
    matches!(block.first(), Some(0x88..=0x8a))
}
