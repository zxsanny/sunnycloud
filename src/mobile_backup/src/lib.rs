//! `mobile_backup`: the one Rust library of the Android and iOS backup apps (ADR-013), on top of `sync-core`.
//! It owns the checkpoint database (17 §4) and is its only writer, and the backup loop (`Backup::run_backup`); the
//! UniFFI business surface comes with E-27/c07.

mod api;
mod backup;
mod checkpoint;
mod config;
mod outcome;
mod source;

pub use api::BackupApi;
pub use backup::Backup;
pub use checkpoint::{CheckpointDb, DbError, SCHEMA_VERSION, SqliteCheckpointStore};
pub use config::AndroidConfig;
pub use outcome::{BackupOutcome, PauseReason, RunContext, RunError, SkipReason, Trigger};
pub use source::{AccessLevel, AssetDescriptor, AssetGone, AssetSource, ChangeToken, ReadableFd};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

/// The crate version, the one export the host calls to prove the library loaded.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn library_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
