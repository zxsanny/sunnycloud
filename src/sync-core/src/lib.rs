//! Native executor of the sunnycloud sync protocol: typed errors and the wire-code table, the host traits,
//! the `sync` settings and the request path every operation uses.

mod config;
mod error;
mod host;
mod request;
mod wire_table;

pub use config::SyncConfig;
pub use error::{Mapped, PlannerInput, SyncError};
pub use host::{
    BatchRecord, CheckpointStore, Credential, CredentialProvider, DownloadSink, FileIdentity,
    FileSource, MemoryCheckpointStore, PersistFailed, ProgressSink, StoreError, UploadCheckpoint,
    UploadState,
};
pub use request::{BoxFuture, Method, Request, RequestPath, Transport, WireRequest, WireResponse};
pub use wire_table::map_problem;
