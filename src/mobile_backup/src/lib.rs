//! `mobile_backup`: the one Rust library of the Android and iOS backup apps (ADR-013), on top of `sync-core`.
//! It owns the checkpoint database (17 §4) and is its only writer, and the backup loop (`Backup::run_backup`); the
//! UniFFI object `BackupCore` (`set_server`, `status`, `cancel`) with the typed errors of the wire table.

mod api;
mod backup;
mod backup_core;
mod checkpoint;
mod config;
mod error;
#[cfg(feature = "uniffi")]
mod ffi;
mod outcome;
mod server;
mod source;
mod status;

pub use api::BackupApi;
pub use backup::Backup;
pub use backup_core::BackupCore;
pub use checkpoint::{CheckpointDb, DbError, SCHEMA_VERSION, SqliteCheckpointStore};
pub use config::AndroidConfig;
pub use error::BackupError;
#[cfg(feature = "uniffi")]
pub use ffi::{ForeignAssetSource, ForeignOpened};
pub use outcome::{
    BackupOutcome, NetworkPolicy, NetworkState, PauseReason, RunContext, RunError, SkipReason,
    Trigger,
};
pub use server::{HttpProbe, ServerCheck, ServerProbe, ServerTrust};
pub use source::{
    AccessLevel, AssetDescriptor, AssetSource, ChangeToken, OpenError, OpenedAsset, ReadableFd,
    SourceError,
};
pub use status::{BackupStatus, LastError, SkippedCounts};

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

/// The crate version, the one export the host calls to prove the library loaded.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn library_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
