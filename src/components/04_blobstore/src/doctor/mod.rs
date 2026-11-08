//! The blobstore checks of `sunnycloud doctor`. The master key check is with the keyring, which owns the key
//! check file. Findings carry ids and sizes, never a path, a file name or key material.

mod missing;
mod retention;
mod volume;

use contracts::{Finding, FindingLevel};

pub use missing::MissingBlobsCheck;
pub use retention::RetentionCheck;
pub use volume::VolumeCheck;

fn finding(level: FindingLevel, message: impl Into<String>, remedy: &str) -> Finding {
    Finding {
        level,
        message: message.into(),
        remedy: remedy.to_owned(),
    }
}

fn ok(message: impl Into<String>) -> Finding {
    finding(FindingLevel::Ok, message, "")
}
