//! `mobile_backup`: the one Rust library of the Android and iOS backup apps (ADR-013), on top of `sync-core`.
//! It owns the checkpoint database (17 §4) and is its only writer; the backup loop and the UniFFI business surface
//! come with E-27/c06 and c07.

mod checkpoint;
mod config;

pub use checkpoint::{CheckpointDb, DbError, SCHEMA_VERSION, SqliteCheckpointStore};
pub use config::AndroidConfig;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

/// The crate version, the one export the host calls to prove the library loaded.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn library_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
