use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::error::{ConfigError, KeyProblem, Reason};
use crate::section::ConfigSection;
use crate::sources::ConfigSources;
use crate::spec::{ENV_PREFIX, KeySpec, Reader};
use crate::table::{Settings, TABLE};
use crate::value::{Environment, SettingType, ValueKind};

/// The one environment variable with the prefix that no binary reads and none refuses.
const TOLERATED_VARIABLE: &str = "SUNNYCLOUD_VERSION";

/// A settings document (config file or client settings JSON) already in memory.
pub(crate) struct Document {
    pub label: String,
    pub text: String,
}

/// Loads a section from compiled defaults, the config files, the environment and secret files.
pub fn load<T: ConfigSection>(sources: &ConfigSources) -> Result<Arc<T>, ConfigError> {
    load_documents_for(sources, &read_documents(sources)?, None)
}

/// Like [`load`], for a command that uses only the `wanted` table keys. A key outside the list is not
/// bound: nothing about it can refuse the start (not a missing value, a wrong type or range, nor an
/// unreadable secret file), and a cross-field rule of the section counts only when it names a wanted key.
/// Unknown `SUNNYCLOUD_*` variables and unreadable config files still refuse. Every missing wanted
/// secret is listed in one [`ConfigError::Invalid`]; a single missing secret with nothing else wrong is
/// [`ConfigError::MissingSecret`] as in [`load`].
pub fn load_keys<T: ConfigSection>(
    sources: &ConfigSources,
    wanted: &[&str],
) -> Result<Arc<T>, ConfigError> {
    load_documents_for(sources, &read_documents(sources)?, Some(wanted))
}

fn read_documents(sources: &ConfigSources) -> Result<Vec<Document>, ConfigError> {
    let mut documents = Vec::with_capacity(sources.files.len());
    for path in &sources.files {
        let text = fs::read_to_string(path)
            .map_err(|error| ConfigError::Unreadable(path.clone(), error.kind()))?;
        documents.push(Document {
            label: path.display().to_string(),
            text,
        });
    }
    Ok(documents)
}

pub(crate) fn load_documents<T: ConfigSection>(
    sources: &ConfigSources,
    documents: &[Document],
) -> Result<Arc<T>, ConfigError> {
    load_documents_for(sources, documents, None)
}

fn load_documents_for<T: ConfigSection>(
    sources: &ConfigSources,
    documents: &[Document],
    wanted: Option<&[&str]>,
) -> Result<Arc<T>, ConfigError> {
    let mut binder = Binder::new(T::READER, sources, documents, wanted);
    let settings = Settings::bind(&mut binder);
    binder.finish(T::from_settings(settings))
}

pub(crate) struct Binder<'a> {
    reader: Reader,
    sources: &'a ConfigSources,
    wanted: Option<&'a [&'a str]>,
    environment: Environment,
    /// Text per key after the file and environment layers.
    layered: BTreeMap<String, String>,
    problems: Vec<KeyProblem>,
    missing_secrets: Vec<String>,
    unreadable: Option<ConfigError>,
}

impl<'a> Binder<'a> {
    fn new(
        reader: Reader,
        sources: &'a ConfigSources,
        documents: &[Document],
        wanted: Option<&'a [&'a str]>,
    ) -> Self {
        let mut binder = Binder {
            reader,
            sources,
            wanted,
            environment: sources.environment,
            layered: BTreeMap::new(),
            problems: Vec::new(),
            missing_secrets: Vec::new(),
            unreadable: None,
        };
        for document in documents {
            binder.layer_document(document);
        }
        binder.layer_environment();
        binder.apply_release_fixed();
        binder.resolve_environment();
        binder
    }

    fn problem(&mut self, key: &str, reason: Reason) {
        self.problems.push(KeyProblem::new(key, reason));
    }

    fn is_wanted(&self, key: &str) -> bool {
        self.wanted.is_none_or(|wanted| wanted.contains(&key))
    }

    /// Problems of a table key the command does not use are dropped; unknown names stay.
    fn counts(&self, key: &str) -> bool {
        self.is_wanted(key) || !TABLE.iter().any(|spec| spec.key == key)
    }

    fn layer_document(&mut self, document: &Document) {
        let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(&document.text) else {
            self.problem(&document.label, Reason::WrongType);
            return;
        };
        for (key, value) in entries {
            if !TABLE.iter().any(|spec| spec.key == key) {
                self.problem(&key, Reason::Unknown);
            } else if let Some(text) = value_text(&value) {
                self.layered.insert(key, text);
            } else {
                self.problem(&key, Reason::WrongType);
            }
        }
    }

    fn layer_environment(&mut self) {
        for (name, value) in &self.sources.env {
            if !name.starts_with(ENV_PREFIX) || name == TOLERATED_VARIABLE {
                continue;
            }
            match TABLE.iter().find(|spec| spec.env_name() == *name) {
                Some(spec) => {
                    self.layered.insert(spec.key.to_owned(), value.clone());
                }
                None if TABLE.iter().any(|s| s.secret && s.file_env_name() == *name) => {}
                None => self
                    .problems
                    .push(KeyProblem::new(name.as_str(), Reason::Unknown)),
            }
        }
    }

    /// In a release build the compiled values replace whatever the file and environment said.
    fn apply_release_fixed(&mut self) {
        let Some(fixed) = &self.sources.release_fixed else {
            return;
        };
        for spec in TABLE.iter().filter(|spec| spec.release_fixed) {
            match fixed.get(spec.key) {
                Some(value) => {
                    self.layered.insert(spec.key.to_owned(), value.clone());
                }
                None => {
                    self.layered.remove(spec.key);
                    if spec.read_by.contains(self.reader) {
                        self.problems
                            .push(KeyProblem::new(spec.key, Reason::MissingRequired));
                    }
                }
            }
        }
    }

    /// `ConfigSources::environment` is the starting value of the `environment` key; file and
    /// environment variable override it. A malformed value is reported when the key is bound.
    fn resolve_environment(&mut self) {
        let text = self
            .layered
            .entry("environment".to_owned())
            .or_insert_with(|| self.sources.environment.as_str().to_owned());
        self.environment = Environment::parse(text).unwrap_or(self.sources.environment);
    }

    pub(crate) fn bind<T: SettingType>(&mut self, spec: &KeySpec) -> T {
        if !spec.read_by.contains(self.reader) || !self.is_wanted(spec.key) {
            return spec
                .default
                .and_then(|default| T::parse(default, spec))
                .unwrap_or_default();
        }
        let from_secret = if spec.secret {
            self.read_secret(spec)
        } else {
            None
        };
        let layered = if spec.secret {
            None
        } else {
            self.layered.get(spec.key).cloned()
        };
        let text = from_secret
            .or(layered)
            .or_else(|| spec.default.map(str::to_owned));
        let Some(text) = text else {
            if !T::OPTIONAL && spec.secret {
                self.missing_secrets.push(spec.key.to_owned());
            } else if !T::OPTIONAL {
                self.problem(spec.key, Reason::MissingRequired);
            }
            return T::default();
        };
        match T::parse(&text, spec) {
            Some(value) => {
                self.check_range(spec, &value);
                value
            }
            None => {
                self.problem(spec.key, Reason::WrongType);
                T::default()
            }
        }
    }

    /// `SUNNYCLOUD_<KEY>_FILE`, else `<secrets_dir>/<key>`. An empty file counts as absent.
    fn read_secret(&mut self, spec: &KeySpec) -> Option<String> {
        let (path, explicit) = match self.sources.env.get(&spec.file_env_name()) {
            Some(path) => (PathBuf::from(path), true),
            None => (self.sources.secrets_dir.join(spec.key), false),
        };
        match fs::read_to_string(&path) {
            Ok(text) => {
                let text = text.trim_end_matches(['\r', '\n']);
                (!text.is_empty()).then(|| text.to_owned())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && !explicit => None,
            Err(error) => {
                self.unreadable
                    .get_or_insert(ConfigError::Unreadable(path, error.kind()));
                None
            }
        }
    }

    /// Durations bind only in production; counts and sizes always.
    fn check_range<T: SettingType>(&mut self, spec: &KeySpec, value: &T) {
        let Some((min_text, max_text)) = spec.range else {
            return;
        };
        let is_duration = spec.kind == ValueKind::Duration;
        if is_duration && self.environment != Environment::Production {
            return;
        }
        let Some(number) = value.number() else {
            return;
        };
        let (Some(min), Some(max)) = (
            spec.kind.parse_number(min_text),
            spec.kind.parse_number(max_text),
        ) else {
            self.problem(spec.key, Reason::WrongType);
            return;
        };
        let reason = if number < min && is_duration {
            Reason::ForbiddenInRelease
        } else if number < min || number > max {
            Reason::OutOfRange {
                min: min_text.to_owned(),
                max: max_text.to_owned(),
            }
        } else {
            return;
        };
        self.problem(spec.key, reason);
    }

    fn finish<T: ConfigSection>(mut self, section: T) -> Result<Arc<T>, ConfigError> {
        if let Some(error) = self.unreadable {
            return Err(error);
        }
        for problem in section.validate(self.environment) {
            if !self.problems.iter().any(|known| known.key == problem.key) {
                self.problems.push(problem);
            }
        }
        if self.wanted.is_some() {
            let kept: Vec<KeyProblem> = std::mem::take(&mut self.problems)
                .into_iter()
                .filter(|problem| self.counts(&problem.key))
                .collect();
            self.problems = kept;
        }
        if self.problems.is_empty() && (self.wanted.is_none() || self.missing_secrets.len() < 2) {
            return match self.missing_secrets.into_iter().next() {
                Some(key) => Err(ConfigError::MissingSecret(key)),
                None => Ok(Arc::new(section)),
            };
        }
        for key in self.missing_secrets {
            self.problems
                .push(KeyProblem::new(key, Reason::MissingRequired));
        }
        Err(ConfigError::Invalid(self.problems))
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// A JSON scalar as its text; an array of scalars as a comma-separated list.
fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => {
            let parts: Option<Vec<String>> = items.iter().map(scalar_text).collect();
            parts.map(|parts| parts.join(","))
        }
        other => scalar_text(other),
    }
}
