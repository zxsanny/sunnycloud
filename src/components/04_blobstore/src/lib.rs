//! Blobstore (component 04): encrypted blob files, their rows, the GC queue, scrub and the master key.
//!
//! This crate holds the typed configuration section, the conversions of I/O, database and cipher failures
//! into the `BlobError` and `KeyError` of `contracts`, and the blob file format: path, header, layout
//! arithmetic, the per-blob data key and the per-chunk sealing, and the group-commit `fdatasync` barrier. The ports are implemented by later tasks. The
//! schema is `src/migrations/*_create_blob_tables.*.sql`.

mod barrier;
mod chunk;
mod config;
mod data_key;
mod error;
mod format_error;
mod header;
mod layout;
mod path;

pub use barrier::SyncBarrier;
#[cfg(feature = "sync-hook")]
pub use barrier::{SyncHook, SyncRecord};
pub use chunk::ChunkCipher;
pub use config::BlobstoreConfig;
pub use data_key::{DATA_KEY_LEN, DataKey};
pub use error::{
    BlobFault, BlobIds, blob_error_from_chunk, blob_error_from_db, blob_error_from_io,
    blob_error_from_unwrap, key_error_from_cipher, key_error_from_db, key_error_from_io,
};
pub use format_error::FormatError;
pub use header::{FORMAT_VERSION, HEADER_LEN, Header, NONCE_PREFIX_LEN, NoncePrefix};
pub use layout::{BlobLayout, CHUNK_SIZE, COUNTER_LIMIT, RangeSpan, SEALED_CHUNK_LEN, TAG_LEN};
pub use path::{blob_path, parse_blob_id};
