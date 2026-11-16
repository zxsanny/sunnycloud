//! `--at` style arguments: an RFC 3339 timestamp that names UTC (`2026-11-01T10:00:00Z`).

use chrono::DateTime;
use sc_clock::Timestamp;

use crate::error::CliError;

pub(crate) fn parse(option: &str, text: &str) -> Result<Timestamp, CliError> {
    let bad = || {
        CliError::Usage(format!(
            "{option} `{text}` is not an RFC 3339 UTC time such as 2026-11-01T10:00:00Z"
        ))
    };
    if !text.ends_with(['Z', 'z']) {
        return Err(bad());
    }
    let parsed = DateTime::parse_from_rfc3339(text).map_err(|_| bad())?;
    Ok(Timestamp::from_unix_micros(parsed.timestamp_micros()))
}
