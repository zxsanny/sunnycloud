//! The frames of `realtime.md`: their packbin schemes and the two conversions the connection needs, a client frame
//! from bytes and a server frame to bytes.
//!
//! packbin's `MapScheme` and `Values` are not `Send` (they hold `Rc<str>`), so the schemes live in a
//! `thread_local!` and every conversion is a plain function: no scheme or value ever crosses an `.await`.

use packbin::{
    MapScheme, PackError, UnpackError, Value, Values, bytes, flags, group, insert, pack, u8, u16,
    u32, u64, unpack, utf8,
};
use uuid::Uuid;

pub(super) const SUBSCRIBE: u8 = 0x01;
pub(super) const PING: u8 = 0x02;
pub(super) const HELLO: u8 = 0x20;
pub(super) const CHANGE: u8 = 0x21;
pub(super) const RESET: u8 = 0x23;
pub(super) const PONG: u8 = 0x24;

/// The version in `Hello`; a released frame never changes its field list (`realtime.md`).
pub(super) const PROTOCOL_VERSION: u16 = 1;

/// A frame the client may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientFrame {
    Subscribe { epoch: u64, seq: u64 },
    Ping { nonce: u32 },
}

/// Why a client message is not a frame. Each is a close 1002 whose reason is a fixed word, never the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FrameError {
    Empty,
    /// The first byte numbers no frame at all.
    UnknownType(u8),
    /// A frame number the server sends, never the client.
    ServerOnlyType(u8),
    /// packbin refused the bytes; the text is its `Debug`, for the log only.
    Malformed(String),
}

impl FrameError {
    pub(super) const fn reason(&self) -> &'static str {
        match self {
            Self::Empty => "empty frame",
            Self::UnknownType(_) => "unknown frame type",
            Self::ServerOnlyType(_) => "server frame type",
            Self::Malformed(_) => "malformed frame",
        }
    }
}

/// `Reset.reason`: the three `ResyncReason` values of `GET /changes`, and a connection that lost pushed events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResetReason {
    EpochChanged = 1,
    BelowPurgeHorizon = 2,
    CursorAhead = 3,
}

/// One node's latest state, as `Change` carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChangeFrame {
    pub epoch: u64,
    pub seq: u64,
    pub node_id: Uuid,
    pub folder: bool,
    pub thumb: u8,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub revision_id: Option<Uuid>,
    pub size: Option<u64>,
    pub content_hash: Option<[u8; 32]>,
    pub media_type: Option<String>,
    pub deleted: bool,
    pub end: bool,
}

struct Schemes {
    subscribe: MapScheme,
    ping: MapScheme,
    hello: MapScheme,
    change: MapScheme,
    reset: MapScheme,
    pong: MapScheme,
}

/// Field ids are the position of a value; a flags anchor is the id of its first member (packbin 0.5.1).
fn schemes() -> Schemes {
    Schemes {
        subscribe: MapScheme::new(i32::from(SUBSCRIBE), vec![u64("epoch"), u64("seq")]),
        ping: MapScheme::new(i32::from(PING), vec![u32("nonce")]),
        hello: MapScheme::new(
            i32::from(HELLO),
            vec![
                u16("version"),
                u16("ping_interval_s"),
                u32("max_client_frame"),
            ],
        ),
        change: MapScheme::new(
            i32::from(CHANGE),
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
        ),
        reset: MapScheme::new(
            i32::from(RESET),
            vec![u8("reason"), u64("epoch"), u64("seq")],
        ),
        pong: MapScheme::new(i32::from(PONG), vec![u32("nonce")]),
    }
}

thread_local! {
    static SCHEMES: Schemes = schemes();
}

/// The client frame in `message`.
pub(super) fn decode_client(message: &[u8]) -> Result<ClientFrame, FrameError> {
    let Some(&number) = message.first() else {
        return Err(FrameError::Empty);
    };
    SCHEMES.with(|schemes| match number {
        SUBSCRIBE => {
            let values = read(&schemes.subscribe, message)?;
            Ok(ClientFrame::Subscribe {
                epoch: number_u64(&values, "epoch")?,
                seq: number_u64(&values, "seq")?,
            })
        }
        PING => {
            let values = read(&schemes.ping, message)?;
            Ok(ClientFrame::Ping {
                nonce: number_u32(&values, "nonce")?,
            })
        }
        HELLO..=0x3f => Err(FrameError::ServerOnlyType(number)),
        _ => Err(FrameError::UnknownType(number)),
    })
}

pub(super) fn encode_hello(
    ping_interval_s: u16,
    max_client_frame: u32,
) -> Result<Vec<u8>, PackError> {
    SCHEMES.with(|schemes| {
        let mut values = Values::new();
        insert(&mut values, "version", Some(Value::U16(PROTOCOL_VERSION)));
        insert(
            &mut values,
            "ping_interval_s",
            Some(Value::U16(ping_interval_s)),
        );
        insert(
            &mut values,
            "max_client_frame",
            Some(Value::U32(max_client_frame)),
        );
        pack(&schemes.hello, &values)
    })
}

pub(super) fn encode_pong(nonce: u32) -> Result<Vec<u8>, PackError> {
    SCHEMES.with(|schemes| {
        let mut values = Values::new();
        insert(&mut values, "nonce", Some(Value::U32(nonce)));
        pack(&schemes.pong, &values)
    })
}

pub(super) fn encode_reset(
    reason: ResetReason,
    epoch: u64,
    seq: u64,
) -> Result<Vec<u8>, PackError> {
    SCHEMES.with(|schemes| {
        let mut values = Values::new();
        insert(&mut values, "reason", Some(Value::U8(reason as u8)));
        insert(&mut values, "epoch", Some(Value::U64(epoch)));
        insert(&mut values, "seq", Some(Value::U64(seq)));
        pack(&schemes.reset, &values)
    })
}

pub(super) fn encode_change(frame: &ChangeFrame) -> Result<Vec<u8>, PackError> {
    SCHEMES.with(|schemes| {
        let mut values = Values::new();
        let mut put = |name: &str, value: Value| insert(&mut values, name, Some(value));
        put("epoch", Value::U64(frame.epoch));
        put("seq", Value::U64(frame.seq));
        put("node_id", Value::Bytes(frame.node_id.as_bytes().to_vec()));
        put("kind", Value::U8(u8::from(frame.folder)));
        put("thumb", Value::U8(frame.thumb));
        put("name", Value::Str(frame.name.clone()));
        if let Some(id) = frame.parent_id {
            put("parent_id", Value::Bytes(id.as_bytes().to_vec()));
        }
        if let Some(id) = frame.revision_id {
            put("revision_id", Value::Bytes(id.as_bytes().to_vec()));
        }
        if let Some(size) = frame.size {
            put("size", Value::U64(size));
        }
        if let Some(hash) = frame.content_hash {
            put("content_hash", Value::Bytes(hash.to_vec()));
        }
        if let Some(media_type) = &frame.media_type {
            put("media_type", Value::Str(media_type.clone()));
        }
        if frame.deleted {
            put("deleted", Value::U8(1));
        }
        if frame.end {
            put("end", Value::U8(1));
        }
        pack(&schemes.change, &values)
    })
}

fn read(scheme: &MapScheme, message: &[u8]) -> Result<Values, FrameError> {
    unpack(scheme, message)
        .map_err(|error: UnpackError| FrameError::Malformed(format!("{error:?}")))
}

fn number_u64(values: &Values, name: &str) -> Result<u64, FrameError> {
    match values.get(name) {
        Some(Some(Value::U64(number))) => Ok(*number),
        other => Err(FrameError::Malformed(format!("{name}: {other:?}"))),
    }
}

fn number_u32(values: &Values, name: &str) -> Result<u32, FrameError> {
    match values.get(name) {
        Some(Some(Value::U32(number))) => Ok(*number),
        other => Err(FrameError::Malformed(format!("{name}: {other:?}"))),
    }
}

#[cfg(test)]
mod tests;
