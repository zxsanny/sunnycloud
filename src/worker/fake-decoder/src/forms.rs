//! The file forms of script values that have no plain serde shape: durations in milliseconds, garbage bytes as
//! chunks, and a `hello` that names only the capabilities it changes.

use std::time::Duration;

use media_shared::decoder_protocol::Capabilities;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use crate::hostile::frame;
use crate::script::capabilities;

/// Garbage larger than this is refused: the cases are about one frame of 64 KiB, not about memory.
const MAX_GARBAGE: usize = 16 << 20;

pub(crate) fn millis<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    u64::deserialize(deserializer).map(Duration::from_millis)
}

pub(crate) fn optional_millis<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Duration>, D::Error> {
    Option::<u64>::deserialize(deserializer).map(|ms| ms.map(Duration::from_millis))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GarbageForm {
    #[serde(default)]
    framed: bool,
    chunks: Vec<Chunk>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Chunk {
    Hex(String),
    Text(String),
    Repeat { byte: u8, count: usize },
}

fn parse_hex(hex: &str) -> Result<Vec<u8>, String> {
    // `from_str_radix` alone would take a leading `+`.
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) || !hex.len().is_multiple_of(2) {
        return Err(format!("hex {hex:?} is not an even number of hex digits"));
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&hex[at..at + 2], 16)
                .map_err(|_| format!("hex {hex:?} holds a non-hex digit"))
        })
        .collect()
}

/// `{"framed": bool, "chunks": [...]}`: the chunks joined in order, behind a length prefix when `framed`.
pub(crate) fn garbage<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let form = GarbageForm::deserialize(deserializer)?;
    let mut bytes = Vec::new();
    for chunk in form.chunks {
        match chunk {
            Chunk::Hex(hex) => bytes.extend(parse_hex(&hex).map_err(D::Error::custom)?),
            Chunk::Text(text) => bytes.extend_from_slice(text.as_bytes()),
            Chunk::Repeat { byte, count } => {
                if bytes.len().saturating_add(count) > MAX_GARBAGE {
                    return Err(D::Error::custom("garbage is larger than 16 MiB"));
                }
                bytes.extend(std::iter::repeat_n(byte, count));
            }
        }
        if bytes.len() > MAX_GARBAGE {
            return Err(D::Error::custom("garbage is larger than 16 MiB"));
        }
    }
    Ok(if form.framed { frame(&bytes) } else { bytes })
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Hello {
    protocol: u32,
    vips: String,
    ffmpeg: String,
    libheif: String,
    build_timestamp: u64,
    nvenc_probe_ok: bool,
    nvenc_sessions: u32,
}

impl Default for Hello {
    fn default() -> Self {
        let defaults = capabilities();
        Self {
            protocol: defaults.protocol,
            vips: defaults.vips,
            ffmpeg: defaults.ffmpeg,
            libheif: defaults.libheif,
            build_timestamp: defaults.build_timestamp,
            nvenc_probe_ok: defaults.nvenc_probe_ok,
            nvenc_sessions: defaults.nvenc_sessions,
        }
    }
}

/// The capabilities fields the script names; every other field keeps its value from [`capabilities`].
pub(crate) fn partial_hello<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Capabilities, D::Error> {
    let hello = Hello::deserialize(deserializer)?;
    Ok(Capabilities {
        protocol: hello.protocol,
        vips: hello.vips,
        ffmpeg: hello.ffmpeg,
        libheif: hello.libheif,
        build_timestamp: hello.build_timestamp,
        nvenc_probe_ok: hello.nvenc_probe_ok,
        nvenc_sessions: hello.nvenc_sessions,
    })
}
