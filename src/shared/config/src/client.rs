use std::sync::{Arc, PoisonError, RwLock};

use crate::bind::{Document, load, load_documents};
use crate::error::ConfigError;
use crate::section::ConfigSection;
use crate::sources::ConfigSources;

/// The active settings of a client. Readers take an `Arc`; a reload swaps the whole `Arc`.
pub struct ClientSettingsHandle<T> {
    sources: ConfigSources,
    current: RwLock<Arc<T>>,
}

impl<T: ConfigSection> ClientSettingsHandle<T> {
    /// Initial load from `sources`.
    pub fn new(sources: ConfigSources) -> Result<Self, ConfigError> {
        let current = load::<T>(&sources)?;
        Ok(Self {
            sources,
            current: RwLock::new(current),
        })
    }

    /// The settings in force now.
    pub fn current(&self) -> Arc<T> {
        // The guarded value is one `Arc` that is replaced in a single store, so a panic in
        // another holder cannot leave it half-written: a poisoned lock is safe to read.
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Applies a settings document (a JSON object keyed by table key) in place of the config
    /// files, keeping environment and secrets. On error the previous settings stay active.
    pub fn reload_client_settings(&self, json: &str) -> Result<Arc<T>, ConfigError> {
        let document = Document {
            label: "settings document".to_owned(),
            text: json.to_owned(),
        };
        let next = load_documents::<T>(&self.sources, &[document])?;
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&next);
        Ok(next)
    }
}
