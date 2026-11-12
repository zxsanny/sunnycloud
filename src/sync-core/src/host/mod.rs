//! The five traits through which the core reaches host state (`host_traits.md`). Methods are synchronous:
//! they are plain callbacks that UniFFI can bind, and the executors call blocking ones from a blocking task.

mod checkpoint;
mod credential;
mod io;
mod memory_store;

pub use checkpoint::{BatchRecord, CheckpointStore, StoreError, UploadCheckpoint, UploadState};
pub use credential::{Credential, CredentialProvider, CredentialUnavailable, PersistFailed};
pub use io::{DownloadSink, FileIdentity, FileSource, ProgressSink};
pub use memory_store::MemoryCheckpointStore;
