use std::fmt;

/// Longest legal name in bytes of UTF-8 (assumption A1).
pub const MAX_NAME_BYTES: usize = 255;

/// Why a name is rejected. The server rejects only these classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum NameError {
    #[error("name is empty")]
    Empty,
    #[error("name is {0} bytes, the limit is 255")]
    TooLong(usize),
    #[error("name is `.`")]
    Dot,
    #[error("name is `..`")]
    DotDot,
    #[error("name contains `/`")]
    ContainsSlash,
    #[error("name contains NUL")]
    ContainsNul,
    #[error("name contains a control character")]
    ControlChar,
    #[error("name contains a bidirectional control character")]
    BidiControl,
}

/// A name that passed [`validate_name`]: 1 to 255 bytes of UTF-8, original case kept.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ValidName(String);

impl ValidName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Callers guarantee the invariants by construction and assert them with `debug_assert!`.
    pub(crate) fn new_unchecked(s: String) -> Self {
        debug_assert!(validate_name(&s).is_ok(), "invalid name built: {s:?}");
        Self(s)
    }
}

impl AsRef<str> for ValidName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ValidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bidi controls rejected by the server: U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069.
pub(crate) fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Accepts every name except: empty, over 255 bytes, whole name `.` or `..`, and names containing `/`,
/// NUL, another Unicode Cc control character or a bidi control. When several classes apply, the length
/// check wins, then `.`/`..`, then the first offending character in reading order.
pub fn validate_name(s: &str) -> Result<ValidName, NameError> {
    if s.is_empty() {
        return Err(NameError::Empty);
    }
    if s.len() > MAX_NAME_BYTES {
        return Err(NameError::TooLong(s.len()));
    }
    match s {
        "." => return Err(NameError::Dot),
        ".." => return Err(NameError::DotDot),
        _ => {}
    }
    for c in s.chars() {
        if c == '/' {
            return Err(NameError::ContainsSlash);
        }
        if c == '\0' {
            return Err(NameError::ContainsNul);
        }
        if c.is_control() {
            return Err(NameError::ControlChar);
        }
        if is_bidi_control(c) {
            return Err(NameError::BidiControl);
        }
    }
    Ok(ValidName(s.to_owned()))
}
