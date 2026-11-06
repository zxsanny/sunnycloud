//! The daemon's SQLite index (14 §2 `LocalIndexStore`, data model §11): pairs, entries, settings, the one feed
//! cursor, and the sync-core `CheckpointStore`. It is an index, never the truth: a lost or damaged file is
//! replaced by [`LocalIndexStore::rebuild`], then a rescan and the first-sync merge relink it and delete nothing.

mod checkpoint;
mod error;
mod model;
mod path_key;
pub(crate) mod repo;
mod store;
#[cfg(test)]
mod tests;

pub use error::IndexError;
pub use model::{Entry, EntryState, NewPair, Pair, PairState};
pub use repo::migrate::SCHEMA_VERSION;
pub use store::{INDEX_FILE_NAME, LocalIndexStore};
