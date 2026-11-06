//! Blobstore (component 04): encrypted blob files, their rows, the GC queue, scrub and the master key.
//!
//! This crate holds the typed configuration section and the conversions of I/O, database and cipher failures
//! into the `BlobError` and `KeyError` of `contracts`; the ports are implemented by later tasks. The schema is
//! `src/migrations/*_create_blob_tables.*.sql`.

mod config;
mod error;

pub use config::BlobstoreConfig;
pub use error::{
    BlobFault, BlobIds, blob_error_from_chunk, blob_error_from_db, blob_error_from_io,
    blob_error_from_unwrap, key_error_from_cipher, key_error_from_db, key_error_from_io,
};
