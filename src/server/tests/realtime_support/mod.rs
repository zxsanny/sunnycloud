//! A WebSocket client and the frame decoder of `realtime.md` for the pipeline tests: the schemes are written out
//! again here, from the contract, so a drift between them and the server's is a failing test.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use packbin::{
    MapScheme, Value, Values, bytes, flags, group, to_hex, u8, u16, u32, u64, unpack, utf8,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{WebSocketStream, client_async};
use uuid::Uuid;

pub const ORIGIN: &str = "https://cloud.example.org";
pub const COOKIE: &str = "__Host-session";
pub const WITHIN: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub epoch: u64,
    pub seq: u64,
    pub node_id: Uuid,
    pub folder: bool,
    pub thumb: u8,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub revision_id: Option<Uuid>,
    pub size: Option<u64>,
    pub content_hash: Option<String>,
    pub media_type: Option<String>,
    pub deleted: bool,
    pub end: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Hello {
        version: u16,
        ping_interval_s: u16,
        max_client_frame: u32,
    },
    Change(Change),
    ThumbState {
        content_hash: String,
        state: u8,
    },
    Reset {
        reason: u8,
        epoch: u64,
        seq: u64,
    },
    Pong {
        nonce: u32,
    },
}

fn get<'a>(values: &'a Values, name: &str) -> Option<&'a Value> {
    values.get(name).and_then(Option::as_ref)
}

fn number(values: &Values, name: &str) -> u64 {
    match get(values, name) {
        Some(Value::U8(n)) => u64::from(*n),
        Some(Value::U16(n)) => u64::from(*n),
        Some(Value::U32(n)) => u64::from(*n),
        Some(Value::U64(n)) => *n,
        other => panic!("{name}: {other:?}"),
    }
}

fn id(values: &Values, name: &str) -> Option<Uuid> {
    match get(values, name) {
        Some(Value::Bytes(raw)) => Some(Uuid::from_slice(raw).expect("16 bytes")),
        Some(other) => panic!("{name}: {other:?}"),
        None => None,
    }
}

fn hex(values: &Values, name: &str) -> Option<String> {
    match get(values, name) {
        Some(Value::Bytes(raw)) => Some(to_hex(raw)),
        Some(other) => panic!("{name}: {other:?}"),
        None => None,
    }
}

fn text(values: &Values, name: &str) -> Option<String> {
    match get(values, name) {
        Some(Value::Str(text)) => Some(text.clone()),
        Some(other) => panic!("{name}: {other:?}"),
        None => None,
    }
}

pub fn decode(frame: &[u8]) -> Frame {
    let read = |scheme: MapScheme| {
        unpack(&scheme, frame).unwrap_or_else(|e| panic!("{e:?}: {frame:02x?}"))
    };
    match frame[0] {
        0x20 => {
            let v = read(MapScheme::new(
                0x20,
                vec![
                    u16("version"),
                    u16("ping_interval_s"),
                    u32("max_client_frame"),
                ],
            ));
            Frame::Hello {
                version: number(&v, "version") as u16,
                ping_interval_s: number(&v, "ping_interval_s") as u16,
                max_client_frame: number(&v, "max_client_frame") as u32,
            }
        }
        0x21 => {
            let v = read(MapScheme::new(
                0x21,
                vec![
                    u64("epoch"),
                    u64("seq"),
                    bytes("node_id", 16),
                    u8("kind"),
                    u8("thumb"),
                    utf8("name"),
                    flags(
                        6,
                        "present",
                        vec![
                            bytes("parent_id", 16),
                            bytes("revision_id", 16),
                            u64("size"),
                            bytes("content_hash", 32),
                            utf8("media_type"),
                            group(11, "deleted", vec![]),
                            group(12, "end", vec![]),
                        ],
                    ),
                ],
            ));
            Frame::Change(Change {
                epoch: number(&v, "epoch"),
                seq: number(&v, "seq"),
                node_id: id(&v, "node_id").expect("node id"),
                folder: number(&v, "kind") == 1,
                thumb: number(&v, "thumb") as u8,
                name: text(&v, "name").expect("name"),
                parent_id: id(&v, "parent_id"),
                revision_id: id(&v, "revision_id"),
                size: get(&v, "size").map(|_| number(&v, "size")),
                content_hash: hex(&v, "content_hash"),
                media_type: text(&v, "media_type"),
                deleted: get(&v, "deleted").is_some(),
                end: get(&v, "end").is_some(),
            })
        }
        0x22 => {
            let v = read(MapScheme::new(
                0x22,
                vec![bytes("content_hash", 32), u8("state")],
            ));
            Frame::ThumbState {
                content_hash: hex(&v, "content_hash").expect("hash"),
                state: number(&v, "state") as u8,
            }
        }
        0x23 => {
            let v = read(MapScheme::new(
                0x23,
                vec![u8("reason"), u64("epoch"), u64("seq")],
            ));
            Frame::Reset {
                reason: number(&v, "reason") as u8,
                epoch: number(&v, "epoch"),
                seq: number(&v, "seq"),
            }
        }
        0x24 => {
            let v = read(MapScheme::new(0x24, vec![u32("nonce")]));
            Frame::Pong {
                nonce: number(&v, "nonce") as u32,
            }
        }
        other => panic!("unknown server frame {other:#x}"),
    }
}

pub fn subscribe(epoch: u64, seq: u64) -> Vec<u8> {
    let mut frame = vec![0x01];
    frame.extend(epoch.to_le_bytes());
    frame.extend(seq.to_le_bytes());
    frame
}

pub fn ping(nonce: u32) -> Vec<u8> {
    let mut frame = vec![0x02];
    frame.extend(nonce.to_le_bytes());
    frame
}

/// What a handshake that is not a 101 answered.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Refused {
    pub fn code(&self) -> String {
        serde_json::from_slice::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|body| body["code"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

pub struct Socket {
    ws: WebSocketStream<TcpStream>,
}

impl Socket {
    /// A handshake with exactly `headers`.
    pub async fn open(addr: SocketAddr, headers: &[(&str, String)]) -> Result<Socket, Refused> {
        let mut request = "ws://cloud.example.org/api/v1/realtime"
            .into_client_request()
            .expect("a request");
        for (name, value) in headers {
            request.headers_mut().insert(
                HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        match client_async(request, stream).await {
            Ok((ws, _)) => Ok(Socket { ws }),
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(Refused {
                status: response.status().as_u16(),
                body: response.into_body().unwrap_or_default(),
            }),
            Err(error) => panic!("handshake failed without an answer: {error}"),
        }
    }

    /// The web app's own handshake: the session cookie and the public origin.
    pub async fn web(addr: SocketAddr, token: &str) -> Socket {
        Self::open(addr, &Self::web_headers(token))
            .await
            .expect("the web handshake succeeds")
    }

    pub fn web_headers(token: &str) -> Vec<(&'static str, String)> {
        vec![
            ("Cookie", format!("{COOKIE}={token}")),
            ("Origin", ORIGIN.to_owned()),
        ]
    }

    pub async fn send(&mut self, frame: &[u8]) {
        self.ws
            .send(Message::Binary(frame.to_vec().into()))
            .await
            .expect("send");
    }

    pub async fn send_raw(&mut self, message: Message) {
        self.ws.send(message).await.expect("send");
    }

    pub async fn next(&mut self) -> Option<Message> {
        let waited = tokio::time::timeout(WITHIN, self.ws.next())
            .await
            .expect("a message or the end in time");
        waited.and_then(Result::ok)
    }

    /// The next frame, decoded; control frames are skipped.
    pub async fn frame(&mut self) -> Frame {
        loop {
            match self.next().await {
                Some(Message::Binary(bytes)) => return decode(&bytes),
                Some(Message::Ping(_) | Message::Pong(_)) => {}
                other => panic!("expected a frame, got {other:?}"),
            }
        }
    }

    /// The next `Change` frames until one has `end`, in order.
    pub async fn changes_until_end(&mut self) -> Vec<Change> {
        let mut changes = Vec::new();
        loop {
            match self.frame().await {
                Frame::Change(change) => {
                    let end = change.end;
                    changes.push(change);
                    if end {
                        return changes;
                    }
                }
                other => panic!("expected a Change, got {other:?}"),
            }
        }
    }

    pub async fn close_code(&mut self) -> Option<u16> {
        loop {
            match self.next().await {
                Some(Message::Close(frame)) => return frame.map(|f| u16::from(f.code)),
                Some(_) => {}
                None => return None,
            }
        }
    }

    /// `Hello`, then a `Subscribe` at `(epoch, seq)`.
    pub async fn subscribed(addr: SocketAddr, token: &str, epoch: u64, seq: u64) -> Socket {
        let mut socket = Self::web(addr, token).await;
        assert!(matches!(
            socket.frame().await,
            Frame::Hello { version: 1, .. }
        ));
        socket.send(&subscribe(epoch, seq)).await;
        socket
    }
}
