use serde_json::{Map, Value};

use crate::exit::Exit;
use crate::secret::OneTimeSecret;

/// How a command that ran to the end went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Success,
    Warnings,
    /// The command ran and found problems (`scrub run` mismatch, `doctor` Fail).
    Findings,
}

impl Class {
    pub(crate) const fn exit(self) -> Exit {
        match self {
            Class::Success => Exit::Success,
            Class::Warnings => Exit::Warnings,
            Class::Findings => Exit::Failed,
        }
    }

    /// Value of `result` (and of `code`) in the `--json` envelope.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Class::Success => "ok",
            Class::Warnings => "warnings",
            Class::Findings => "findings",
        }
    }
}

/// What a command hands to the writer: its class, the facts, and at most one one-time secret.
#[derive(Debug)]
pub struct Outcome {
    pub(crate) class: Class,
    pub(crate) data: Map<String, Value>,
    pub(crate) secret: Option<OneTimeSecret>,
}

impl Outcome {
    pub fn new(class: Class) -> Self {
        Self {
            class,
            data: Map::new(),
            secret: None,
        }
    }

    pub fn success() -> Self {
        Self::new(Class::Success)
    }

    pub fn field(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.data.insert(key.to_owned(), value.into());
        self
    }

    pub fn with_secret(mut self, secret: OneTimeSecret) -> Self {
        self.secret = Some(secret);
        self
    }

    pub fn exit(&self) -> Exit {
        self.class.exit()
    }
}
