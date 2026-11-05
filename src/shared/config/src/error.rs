use std::fmt;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Why one key was rejected. Never carries the offending value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Outside the allowed range; `min` and `max` are written as in the key table (`0s`, `90d`).
    OutOfRange { min: String, max: String },
    /// Not parseable as the key's type, or not one of its allowed choices.
    WrongType,
    /// Not a key of the table.
    Unknown,
    /// A required key has no value.
    MissingRequired,
    /// A production start with a value below the duration floor, or with plain `http`.
    ForbiddenInRelease,
}

/// One rejected key. `key` is the table key, or the variable name for unknown variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyProblem {
    pub key: String,
    pub reason: Reason,
}

impl KeyProblem {
    pub fn new(key: impl Into<String>, reason: Reason) -> Self {
        Self {
            key: key.into(),
            reason,
        }
    }
}

impl fmt::Display for KeyProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.key)?;
        match &self.reason {
            Reason::OutOfRange { min, max } => write!(f, "out of range, allowed {min} to {max}"),
            Reason::WrongType => f.write_str("malformed value or wrong type"),
            Reason::Unknown => f.write_str("unknown setting"),
            Reason::MissingRequired => f.write_str("required setting is missing"),
            Reason::ForbiddenInRelease => f.write_str("value is not allowed in production"),
        }
    }
}

fn join_problems(problems: &[KeyProblem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Why a section could not be loaded. Display output never contains a setting value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    /// Every problem of one start, in one error.
    #[error("invalid configuration ({} problems): {}", .0.len(), join_problems(.0))]
    Invalid(Vec<KeyProblem>),
    /// A required secret has no value and nothing else is wrong; carries the key.
    #[error("missing required secret `{0}`")]
    MissingSecret(String),
    /// A named file (config file, `_FILE` target) could not be read.
    #[error("cannot read `{path}`: {kind:?}", path = .0.display(), kind = .1)]
    Unreadable(PathBuf, io::ErrorKind),
}
