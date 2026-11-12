//! `update-manifest.json`, format 1 (ADR-017 Notes, confirmed 2026-11-11). Parsing is strict: an unknown
//! field, format or malformed value is refused, never misread.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

/// The only layout this daemon reads.
pub const FORMAT: u64 = 1;
pub const MANIFEST_NAME: &str = "update-manifest.json";
pub const SIGNATURE_NAME: &str = "update-manifest.json.sig";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u64,
    pub version: String,
    pub serial: u64,
    pub expires: String,
    pub platforms: BTreeMap<String, PlatformEntry>,
}

/// `asset` is a file name below the release of the manifest's tag, never a URL.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformEntry {
    pub asset: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct FormatOnly {
    format: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// Not JSON, or `format` is missing or another value.
    UnknownFormat,
    /// Format 1, but a field is missing, unknown or malformed.
    Malformed,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, LayoutError> {
        let head: FormatOnly =
            serde_json::from_slice(bytes).map_err(|_| LayoutError::UnknownFormat)?;
        if head.format != FORMAT {
            return Err(LayoutError::UnknownFormat);
        }
        serde_json::from_slice(bytes).map_err(|_| LayoutError::Malformed)
    }
}

/// `MAJOR.MINOR.PATCH` with decimal parts only; pre-release and build suffixes are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.').map(|part| {
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse::<u64>().ok()).flatten()
        });
        let version = Self(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `platforms` key of this machine: `<os>-<arch>`, e.g. `linux-x86_64`.
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// A plain file name: no separator, no leading dot, only `[A-Za-z0-9._+-]`, so it cannot leave the
/// release directory or form a URL.
pub fn is_plain_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// Lowercase or uppercase hex of exactly 32 bytes.
pub fn parse_sha256(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        *slot = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` (UTC, no fraction, no offset) to microseconds since the epoch.
pub fn parse_expires(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let shape = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z';
    if !shape {
        return None;
    }
    let field = |from: usize, to: usize| -> Option<i64> {
        let part = &text[from..to];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hour, minute, second) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    let valid = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    if !valid {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    seconds.checked_mul(1_000_000)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
