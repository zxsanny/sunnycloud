//! UniFFI shapes of the types this crate takes from other crates (`uniffi_surface.md`). Identifiers and
//! digests cross the boundary as their wire text (UUID text, 64 lower-case hex characters), JSON values as
//! JSON text, and a listing page token as its opaque text.

use sc_ids::{FileHash, Hash32};
use uuid::Uuid;

use crate::digest_serde::decode_hex;
use crate::tree::PageToken;

/// `client_meta` and `import_hint` cross the boundary as JSON text.
pub type JsonValue = serde_json::Value;

uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |id| id.to_string(),
    try_lift: |text| Ok(Uuid::parse_str(&text)?),
});

uniffi::custom_type!(Hash32, String, {
    remote,
    lower: |hash| hash.to_string(),
    try_lift: |text| Ok(Hash32::from(decode_hex(&text)?)),
});

uniffi::custom_type!(FileHash, String, {
    remote,
    lower: |hash| hash.to_string(),
    try_lift: |text| Ok(FileHash::from(decode_hex(&text)?)),
});

uniffi::custom_type!(JsonValue, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |text| Ok(serde_json::from_str(&text)?),
});

uniffi::custom_newtype!(PageToken, String);
