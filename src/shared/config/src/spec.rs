use std::time::Duration;

use crate::value::{SettingType, ValueKind};

/// The binaries that read a key. `Clients` covers the desktop daemon, `mobile_backup` and 13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// `sunnycloud` (api and CLI).
    Api,
    /// `sunnycloud-worker`.
    Worker,
    /// `decoder-runner`.
    Decoder,
    /// Desktop and mobile clients.
    Clients,
}

/// Set of readers of one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readers(u8);

impl Readers {
    pub const fn of(reader: Reader) -> Self {
        Readers(1 << reader as u8)
    }

    pub const fn or(self, other: Readers) -> Self {
        Readers(self.0 | other.0)
    }

    pub const fn contains(self, reader: Reader) -> bool {
        self.0 & Readers::of(reader).0 != 0
    }
}

/// Reader sets used by the table rows.
pub(crate) mod readers {
    use super::{Reader, Readers};

    pub const A: Readers = Readers::of(Reader::Api);
    pub const W: Readers = Readers::of(Reader::Worker);
    pub const R: Readers = Readers::of(Reader::Decoder);
    pub const C: Readers = Readers::of(Reader::Clients);
    pub const AW: Readers = A.or(W);
    pub const WR: Readers = W.or(R);
    pub const ALL: Readers = A.or(W).or(R).or(C);
}

pub(crate) const ENV_PREFIX: &str = "SUNNYCLOUD_";

/// One row of the key table. Built from the field type and a few facts, see the crate docs.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec {
    pub key: &'static str,
    pub kind: ValueKind,
    /// Compiled default as text, parsed by the same code as any other source.
    pub default: Option<&'static str>,
    pub secret: bool,
    /// No default and no error when absent (the value is `None`).
    pub optional: bool,
    /// `(min, max)` as text in the key's own syntax. Durations bind only in production.
    pub range: Option<(&'static str, &'static str)>,
    /// Allowed values of a text key; empty means free text.
    pub choices: &'static [&'static str],
    pub read_by: Readers,
    /// Compiled into release builds; env and file entries are ignored there.
    pub release_fixed: bool,
}

impl KeySpec {
    pub const fn with_default(default: &'static str, read_by: Readers) -> Self {
        Self::row(Some(default), read_by)
    }

    pub const fn without_default(read_by: Readers) -> Self {
        Self::row(None, read_by)
    }

    const fn row(default: Option<&'static str>, read_by: Readers) -> Self {
        KeySpec {
            key: "",
            kind: ValueKind::Text,
            default,
            secret: false,
            optional: false,
            range: None,
            choices: &[],
            read_by,
            release_fixed: false,
        }
    }

    pub const fn range(mut self, min: &'static str, max: &'static str) -> Self {
        self.range = Some((min, max));
        self
    }

    pub const fn one_of(mut self, choices: &'static [&'static str]) -> Self {
        self.choices = choices;
        self
    }

    pub const fn fixed(mut self) -> Self {
        self.release_fixed = true;
        self
    }

    /// Fills the facts that the field type already knows.
    pub const fn named<T: SettingType>(mut self, key: &'static str) -> Self {
        self.key = key;
        self.kind = T::KIND;
        self.secret = T::SECRET;
        self.optional = T::OPTIONAL;
        self
    }

    /// `SUNNYCLOUD_` plus the key upper-cased.
    pub fn env_name(&self) -> String {
        format!("{ENV_PREFIX}{}", self.key.to_ascii_uppercase())
    }

    /// Name of the variable that holds the path of a secret's file.
    pub fn file_env_name(&self) -> String {
        format!("{}_FILE", self.env_name())
    }

    /// Floor and ceiling of a duration key.
    pub fn duration_range(&self) -> Option<(Duration, Duration)> {
        let (min, max) = self.range.filter(|_| self.kind == ValueKind::Duration)?;
        Some((
            crate::value::parse_duration(min)?,
            crate::value::parse_duration(max)?,
        ))
    }
}
