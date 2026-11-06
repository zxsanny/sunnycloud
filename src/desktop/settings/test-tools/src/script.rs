//! Scripted answers of the stub daemon. A script is a JSON file (or a value built in code):
//! the initial state the default answers start from, per-method queues of scripted answers,
//! paged listings, and an event schedule.
//!
//! ```json
//! {
//!   "answers": {
//!     "set_server": [
//!       {"error": {"kind": "HttpsRequired"}},
//!       {"delay": {"ms": 4000, "then": {"result": null}}},
//!       "silent",
//!       {"hostile": "invalid_json"},
//!       "default"
//!     ]
//!   }
//! }
//! ```
//!
//! Each call of a method takes the next answer of its queue; an empty queue means the
//! default behaviour (`daemon_control.md`).

use std::collections::BTreeMap;
use std::path::Path;

use ipc_schema::{
    ControlError, DaemonStatus, EscapedEntry, FolderEntry, LoginPrompt, METHODS, Settings,
    SkippedEntry,
};
use serde::Deserialize;
use serde_json::Value;

/// Size of the oversize answer of ST-15-1.
pub const TEN_MB: usize = 10 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("script file {path} cannot be read: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("script does not parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("script scripts the unknown method `{0}`")]
    UnknownMethod(String),
}

/// What the stub does with one call.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    /// The default behaviour of the daemon contract.
    Default,
    /// Success with this `result`, which must decode as the method's result type.
    Result(Value),
    /// A `ControlError` in its JSON-RPC mapping (-32000, `data.kind`).
    Error(ControlError),
    /// No frame at all.
    Silent,
    /// A frame a well-behaved daemon never sends.
    Hostile(Hostile),
    /// The inner answer, sent `ms` milliseconds after the request arrived.
    Delay { ms: u64, then: Box<Answer> },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Hostile {
    /// A valid success response whose line is `bytes` long (a string result padded to size).
    Oversize {
        bytes: usize,
    },
    InvalidJson,
    /// A valid response whose id is not the one of the request.
    WrongId,
    /// An `UpgradeRequired` error whose message is markup: `<b>x</b>{@html}<script>`.
    MarkupError,
    /// This text plus `\n`, verbatim.
    Raw(String),
}

impl Hostile {
    pub fn oversize_10mb() -> Self {
        Self::Oversize { bytes: TEN_MB }
    }
}

/// `statuses[i]` is sent as a `StatusEvent` `(i + 1) * interval_ms` after a client's
/// `subscribe`, in order; after the last one the schedule is over.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSchedule {
    pub interval_ms: u64,
    pub statuses: Vec<DaemonStatus>,
}

/// Pages of one listing, keyed by the parent node id (folders) or the pair id (entries);
/// the key of "no parent" and "all pairs" is the empty string.
pub type Pages<T> = BTreeMap<String, Vec<Vec<T>>>;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Script {
    /// Initial `DaemonStatus`; default: no server, signed out, no pairs.
    pub status: Option<DaemonStatus>,
    pub settings: Option<Settings>,
    pub login_prompt: Option<LoginPrompt>,
    pub answers: BTreeMap<String, Vec<Answer>>,
    pub folder_pages: Pages<FolderEntry>,
    pub skipped_pages: Pages<SkippedEntry>,
    pub escaped_pages: Pages<EscapedEntry>,
    pub events: Option<EventSchedule>,
}

impl Script {
    pub fn parse(text: &str) -> Result<Self, ScriptError> {
        let script: Self = serde_json::from_str(text)?;
        script.validate()?;
        Ok(script)
    }

    pub fn from_file(path: &Path) -> Result<Self, ScriptError> {
        let text = std::fs::read_to_string(path).map_err(|source| ScriptError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text)
    }

    /// A misspelled method name would silently never fire, so it is rejected.
    pub fn validate(&self) -> Result<(), ScriptError> {
        match self
            .answers
            .keys()
            .find(|name| !METHODS.iter().any(|m| m.name == name.as_str()))
        {
            Some(name) => Err(ScriptError::UnknownMethod(name.clone())),
            None => Ok(()),
        }
    }

    pub fn with_answers(mut self, method: &str, answers: Vec<Answer>) -> Self {
        self.answers
            .entry(method.to_owned())
            .or_default()
            .extend(answers);
        self
    }
}
