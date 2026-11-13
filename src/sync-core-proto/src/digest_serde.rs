//! Serde helpers for `sc_ids` digests: lower-case hex text on the wire (64 characters).

use std::fmt::Display;

use serde::de::Error;
use serde::{Deserialize, Deserializer, Serializer};

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Why a digest text was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BadDigest {
    #[error("a digest is 64 hex characters")]
    Length,
    #[error("a digest holds only hex characters")]
    NotHex,
}

/// The 32 bytes of a digest written as 64 hex characters (either case).
pub(crate) fn decode_hex(text: &str) -> Result<[u8; 32], BadDigest> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return Err(BadDigest::Length);
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(high), Some(low)) => *slot = (high << 4) | low,
            _ => return Err(BadDigest::NotHex),
        }
    }
    Ok(out)
}

fn decode<T: From<[u8; 32]>, E: Error>(text: &str) -> Result<T, E> {
    decode_hex(text).map(T::from).map_err(E::custom)
}

pub fn serialize<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

pub fn deserialize<'de, T: From<[u8; 32]>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    decode(&String::deserialize(deserializer)?)
}

pub mod option {
    use super::*;

    pub fn serialize<T: Display, S: Serializer>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.collect_str(value),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: From<[u8; 32]>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| decode(&text))
            .transpose()
    }
}

pub mod list {
    use serde::ser::SerializeSeq;

    use super::*;

    pub fn serialize<T: Display, S: Serializer>(
        values: &[T],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            seq.serialize_element(&value.to_string())?;
        }
        seq.end()
    }

    pub fn deserialize<'de, T: From<[u8; 32]>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<T>, D::Error> {
        Vec::<String>::deserialize(deserializer)?
            .iter()
            .map(|text| decode(text))
            .collect()
    }
}
