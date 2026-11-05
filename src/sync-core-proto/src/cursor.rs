use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// A change-feed position. Wire text `E.S` (epoch, then sequence); the total order is `(epoch, seq)`.
///
/// The derived order relies on the field order: `epoch` first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cursor {
    pub epoch: u64,
    pub seq: u64,
}

/// The text is not `E.S` with two unsigned decimal numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("a cursor is two unsigned decimal numbers joined by a dot")]
pub struct BadCursor;

impl Cursor {
    pub const fn new(epoch: u64, seq: u64) -> Self {
        Self { epoch, seq }
    }
}

fn parse_part(text: &str) -> Result<u64, BadCursor> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(BadCursor);
    }
    text.parse().map_err(|_| BadCursor)
}

impl FromStr for Cursor {
    type Err = BadCursor;

    fn from_str(text: &str) -> Result<Self, BadCursor> {
        let (epoch, seq) = text.split_once('.').ok_or(BadCursor)?;
        Ok(Self {
            epoch: parse_part(epoch)?,
            seq: parse_part(seq)?,
        })
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.epoch, self.seq)
    }
}

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
