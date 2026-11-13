use std::fmt;

use wasm_bindgen::prelude::*;

/// Why an export refused its input. JavaScript sees an `Error` whose `name` is [`ExportError::name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// More than `BLOCK_SIZE` bytes passed as one block.
    BlockTooLarge,
    /// A byte array of the wrong length (block hashes not a multiple of 32, a file hash not 32 bytes,
    /// commit-ID random bytes not 10).
    InvalidLength,
    /// A commit-ID time that is not a whole number of milliseconds in `0..2^48`.
    InvalidTime,
    /// A device id that is not a UUID.
    InvalidUuid,
    /// A planner result object with an unknown `kind` or a field of the wrong shape.
    InvalidResult,
}

impl ExportError {
    pub fn name(self) -> &'static str {
        match self {
            Self::BlockTooLarge => "BlockTooLarge",
            Self::InvalidLength => "InvalidLength",
            Self::InvalidTime => "InvalidTime",
            Self::InvalidUuid => "InvalidUuid",
            Self::InvalidResult => "InvalidResult",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::BlockTooLarge => "a block holds at most BLOCK_SIZE (2 MiB) bytes",
            Self::InvalidLength => "byte array of the wrong length",
            Self::InvalidTime => "time must be whole milliseconds since the Unix epoch, below 2^48",
            Self::InvalidUuid => "not a UUID",
            Self::InvalidResult => "unknown result kind or malformed result field",
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name(), self.message())
    }
}

impl std::error::Error for ExportError {}

#[wasm_bindgen(inline_js = "export function named_error(name, message) { \
    const error = new Error(message); error.name = name; return error; }")]
extern "C" {
    fn named_error(name: &str, message: &str) -> JsValue;
}

impl From<ExportError> for JsValue {
    fn from(error: ExportError) -> Self {
        named_error(error.name(), error.message())
    }
}
