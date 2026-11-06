use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use media_shared::decoder_protocol::ViolationKind;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(payload.len())
        .expect("test payloads fit a u32")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn json_frame(value: &Value) -> Vec<u8> {
    frame(value.to_string().as_bytes())
}

/// A fake decoder on a real socket: for every connection it reads one request frame, sends the scripted
/// bytes, then waits for the client to hang up.
pub struct HostilePeer {
    closed: Arc<AtomicUsize>,
}

impl HostilePeer {
    pub fn start(socket: &Path, reply: Vec<u8>) -> Self {
        Self::spawn(socket, reply, false)
    }

    /// Like `start`, but the peer closes the stream right after the reply, as a dying runner does.
    pub fn start_hanging_up(socket: &Path, reply: Vec<u8>) -> Self {
        Self::spawn(socket, reply, true)
    }

    fn spawn(socket: &Path, reply: Vec<u8>, hang_up: bool) -> Self {
        let listener = UnixListener::bind(socket).expect("the fake peer binds");
        let closed = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&closed);
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                tokio::spawn(serve(stream, reply.clone(), Arc::clone(&counter), hang_up));
            }
        });
        Self { closed }
    }

    /// Connections the client has closed after its scripted reply.
    pub fn closed(&self) -> usize {
        self.closed.load(Ordering::SeqCst)
    }
}

async fn serve(mut stream: UnixStream, reply: Vec<u8>, closed: Arc<AtomicUsize>, hang_up: bool) {
    let mut prefix = [0u8; 4];
    if stream.read_exact(&mut prefix).await.is_ok() {
        let mut request = vec![0u8; u32::from_be_bytes(prefix) as usize];
        if stream.read_exact(&mut request).await.is_ok() {
            // A client that drops the connection mid-reply makes the write fail: it hung up, which counts.
            if stream.write_all(&reply).await.is_ok() && !hang_up {
                let mut rest = [0u8; 64];
                while matches!(stream.read(&mut rest).await, Ok(read) if read > 0) {}
            }
        }
    }
    closed.fetch_add(1, Ordering::SeqCst);
}

/// A listener that accepts every connection and closes it at once, recording when: a decoder that is up but
/// cannot be handshaken.
pub struct AcceptAndClose {
    accepted: Arc<Mutex<Vec<Instant>>>,
}

impl AcceptAndClose {
    pub fn start(socket: &Path) -> Self {
        let listener = UnixListener::bind(socket).expect("the fake peer binds");
        let accepted = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&accepted);
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                record
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Instant::now());
                drop(stream);
            }
        });
        Self { accepted }
    }

    pub fn accepted(&self) -> Vec<Instant> {
        self.accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn probe_reply(video_width: u64, duration_ms: i64, kind: &str) -> Value {
    json!({"response": {"probe": {
        "kind": kind, "container": "mp4", "moov_first": true,
        "video": {"codec": "h264", "profile": null, "width": video_width, "height": 1080,
                  "rotation": null, "bitrate": null, "pix_fmt": null, "primaries": null,
                  "transfer": null, "matrix": null},
        "audio": null, "duration_ms": duration_ms}}})
}

/// The hostile replies of IT-09-13 plus the other classes of the parser, as `(name, bytes, violation)`.
/// Each is sent in answer to a `probe`. A range violation is matched by class only (`OutOfRange("")`).
pub fn hostile_cases() -> Vec<(&'static str, Vec<u8>, ViolationKind)> {
    let mut oversize = (65_537u32).to_be_bytes().to_vec();
    oversize.extend(std::iter::repeat_n(b' ', 65_537));
    let mut extra_field = probe_reply(1920, 1000, "video");
    extra_field["response"]["probe"]["surprise"] = json!(1);
    let mut trailing = probe_reply(1920, 1000, "video").to_string().into_bytes();
    trailing.extend_from_slice(b" x");
    vec![
        ("oversize frame", oversize, ViolationKind::Oversize),
        (
            "absurd length prefix",
            u32::MAX.to_be_bytes().to_vec(),
            ViolationKind::Oversize,
        ),
        (
            "unknown field",
            json_frame(&extra_field),
            ViolationKind::Schema,
        ),
        (
            "width 0",
            json_frame(&probe_reply(0, 1000, "video")),
            ViolationKind::OutOfRange(""),
        ),
        (
            "width 4294967295",
            json_frame(&probe_reply(4_294_967_295, 1000, "video")),
            ViolationKind::OutOfRange(""),
        ),
        (
            "negative duration_ms",
            json_frame(&probe_reply(1920, -1, "video")),
            ViolationKind::Schema,
        ),
        (
            "unknown media kind",
            json_frame(&probe_reply(1920, 1000, "audio")),
            ViolationKind::Schema,
        ),
        (
            "invalid UTF-8",
            frame(&[b'{', 0xff, 0xfe, b'}']),
            ViolationKind::NotUtf8,
        ),
        ("not JSON", frame(b"garbage"), ViolationKind::Json),
        ("two values", frame(&trailing), ViolationKind::Json),
        (
            "request instead of reply",
            json_frame(&json!({"request": "cancel"})),
            ViolationKind::WrongDirection,
        ),
        (
            "unsolicited cancelled",
            json_frame(&json!({"response": "cancelled"})),
            ViolationKind::OutOfSequence,
        ),
        (
            "wrong response kind",
            json_frame(&json!({"response": {"self_check": []}})),
            ViolationKind::OutOfSequence,
        ),
    ]
}
