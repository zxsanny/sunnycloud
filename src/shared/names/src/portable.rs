use icu_normalizer::DecomposingNormalizerBorrowed;

use crate::name::{MAX_NAME_BYTES, ValidName};

/// File-system family of a sync root. Detection is done by the desktop daemon, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsProfile {
    WindowsNtfs,
    PosixCaseSensitive,
    /// APFS, HFS+, exFAT.
    CaseInsensitivePreserving,
}

/// Why a valid name cannot be stored as is on a file-system profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9 and the superscript digit forms), with or
    /// without extension.
    ReservedDeviceName,
    /// Windows: the name ends with `.` or a space.
    TrailingDotOrSpace,
    /// Windows: one of `< > : " | ? * \` (the colon is also [`Reason::AlternateDataStream`]).
    ForbiddenChar,
    /// Windows: `:` (`name:stream`).
    AlternateDataStream,
    /// More than 255 units on the profile: UTF-16 units on Windows, UTF-16 units of the decomposed (NFD)
    /// form on case-insensitive-preserving roots (HFS+), bytes on POSIX.
    OverPlatformLimit,
    /// A lone surrogate. A Rust `&str` cannot hold one, so [`portable_check`] never reports it; the
    /// variant exists for callers that convert from platform strings (contract list).
    LoneSurrogate,
    /// Escaping found a sibling with the same key (case difference).
    CaseCollision,
    /// Escaping found a sibling with the same key and the name is not in NFC.
    NfcNfdCollision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Portable {
    Ok,
    NeedsEscape(Vec<Reason>),
}

impl Portable {
    pub fn is_ok(&self) -> bool {
        matches!(self, Portable::Ok)
    }
}

pub(crate) const PLATFORM_UNIT_LIMIT: usize = 255;

pub(crate) fn is_windows_forbidden(c: char) -> bool {
    matches!(c, '<' | '>' | '"' | '|' | '?' | '*' | '\\')
}

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|d| stem.eq_ignore_ascii_case(d))
        || {
            let mut chars = stem.chars();
            let prefix: String = chars.by_ref().take(3).collect();
            let numbered = prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT");
            numbered
                && matches!(
                    chars.next(),
                    Some('1'..='9' | '\u{00B9}' | '\u{00B2}' | '\u{00B3}')
                )
                && chars.next().is_none()
        }
}

/// Units the profile counts, used for the 255 limit.
pub(crate) fn platform_units(s: &str, profile: FsProfile) -> usize {
    match profile {
        FsProfile::WindowsNtfs => s.encode_utf16().count(),
        FsProfile::PosixCaseSensitive => s.len(),
        FsProfile::CaseInsensitivePreserving => DecomposingNormalizerBorrowed::new_nfd()
            .normalize(s)
            .encode_utf16()
            .count(),
    }
}

pub(crate) fn fits_platform(s: &str, profile: FsProfile) -> bool {
    s.len() <= MAX_NAME_BYTES && platform_units(s, profile) <= PLATFORM_UNIT_LIMIT
}

/// Reports every reason the profile cannot hold `name` as is (each at most once, in enum order).
/// Collisions with siblings need the folder and are reported by [`crate::escape_name`].
pub fn portable_check(name: &ValidName, profile: FsProfile) -> Portable {
    let s = name.as_str();
    let mut reasons = Vec::new();
    if profile == FsProfile::WindowsNtfs {
        if is_reserved_device_name(s) {
            reasons.push(Reason::ReservedDeviceName);
        }
        if s.ends_with(['.', ' ']) {
            reasons.push(Reason::TrailingDotOrSpace);
        }
        if s.chars().any(|c| c == ':' || is_windows_forbidden(c)) {
            reasons.push(Reason::ForbiddenChar);
        }
        if s.contains(':') {
            reasons.push(Reason::AlternateDataStream);
        }
    }
    if !fits_platform(s, profile) {
        reasons.push(Reason::OverPlatformLimit);
    }
    if reasons.is_empty() {
        Portable::Ok
    } else {
        Portable::NeedsEscape(reasons)
    }
}
