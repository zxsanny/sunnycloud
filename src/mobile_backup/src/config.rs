//! The typed `android` section (17 §10). Its keys are the `android_*` rows of the shared key table, so loading, ranges,
//! the `SUNNYCLOUD_ANDROID_*` names and client reload are `sc_config`'s; this type is the view the backup reads.
//! `batch_commit_max` is not a key: it is the server's `batch_max_entries`, learnt at connect.

use std::time::Duration;

use sc_config::ClientSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AndroidConfig {
    /// Period of the WorkManager catch-up run.
    pub catchup_interval: Duration,
    /// Run only on an unmetered network.
    pub unmetered_only: bool,
    /// Run only while charging.
    pub charging_only: bool,
    /// Assets hashed ahead of the one uploading.
    pub hash_ahead: u32,
}

impl AndroidConfig {
    /// Reads the `android_*` keys of an already validated section.
    pub fn from_settings(settings: &ClientSettings) -> Self {
        Self {
            catchup_interval: settings.android_catchup_interval.get(),
            unmetered_only: settings.android_unmetered_only,
            charging_only: settings.android_charging_only,
            hash_ahead: u32::try_from(settings.android_hash_ahead).unwrap_or(u32::MAX),
        }
    }
}

#[cfg(test)]
mod tests;
