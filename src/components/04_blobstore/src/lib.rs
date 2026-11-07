//! Blobstore (component 04): encrypted blob files, their rows, the GC queue, scrub and the master key.
//!
//! This crate holds the typed configuration section, the conversions of I/O, database and cipher failures
//! into the `BlobError` and `KeyError` of `contracts`, and the blob file format: path, header, layout
//! arithmetic, the per-blob data key and the per-chunk sealing, the group-commit `fdatasync` barrier, and the master
//! keyring (key check, data-key wrap), and the `BlobWriter` port, the blob rows (`BlobRegistry`, `BlobCatalog`) the GC queue writer (`GcQueue`) and the GC sweeper (`GcSweeper`) the orphan sweeper (`OrphanSweeper`) and the scrubber (`BlobScrubber`, `ScrubJanitor`). The reader (`FileBlobReader`) is the only decrypt path. The other ports are implemented by later tasks. The
//! schema is `src/migrations/*_create_blob_tables.*.sql`.

mod activity;
mod barrier;
mod catalog;
mod chunk;
mod config;
mod data_key;
mod error;
mod format_error;
mod gc_queue;
mod gc_sweeper;
mod header;
mod keyring;
mod layout;
mod orphan_sweeper;
mod path;
mod reader;
mod registry;
mod scrub;
mod writer;

pub use activity::{ActivityGauge, ActivityGuard};
pub use barrier::SyncBarrier;
#[cfg(feature = "sync-hook")]
pub use barrier::{SyncHook, SyncRecord};
pub use catalog::BlobCatalog;
pub use chunk::ChunkCipher;
pub use config::BlobstoreConfig;
pub use data_key::{DATA_KEY_LEN, DataKey};
pub use error::{
    BlobFault, BlobIds, KEY_OPERATOR_TEXT, blob_error_from_chunk, blob_error_from_db,
    blob_error_from_io, blob_error_from_unwrap, key_error_from_cipher, key_error_from_db,
    key_error_from_io, operator_message,
};
pub use format_error::FormatError;
pub use gc_queue::GcQueue;
#[cfg(feature = "sync-hook")]
pub use gc_sweeper::{GcHook, GcHookPoint, GcHookStop};
pub use gc_sweeper::{GcStats, GcSweeper};
pub use header::{FORMAT_VERSION, HEADER_LEN, Header, NONCE_PREFIX_LEN, NoncePrefix};
pub use keyring::MasterKeyring;
pub use layout::{BlobLayout, CHUNK_SIZE, COUNTER_LIMIT, RangeSpan, SEALED_CHUNK_LEN, TAG_LEN};
#[cfg(feature = "sync-hook")]
pub use orphan_sweeper::{OrphanHook, OrphanHookPoint};
pub use orphan_sweeper::{OrphanStats, OrphanSweeper};
pub use path::{blob_path, parse_blob_id};
pub use reader::FileBlobReader;
#[cfg(feature = "sync-hook")]
pub use reader::ReadProbe;
pub use registry::BlobRegistry;
pub use scrub::{BlobScrubber, ScrubCycle, ScrubJanitor, ScrubMetrics};
#[cfg(feature = "sync-hook")]
pub use scrub::{ScrubHook, ScrubPoint};
pub use writer::FileBlobWriter;
