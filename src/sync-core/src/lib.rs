//! Native executor of the sunnycloud sync protocol: typed errors and the wire-code table, the host traits,
//! the `sync` settings, the request path every operation uses, and `HttpSyncClient::connect`.

#[cfg(all(feature = "allow-plain-http", not(debug_assertions)))]
compile_error!("the `allow-plain-http` feature is for debug builds only");

mod client;
mod config;
mod download;
mod error;
mod feed_ops;
mod hash_ops;
mod host;
mod http_transport;
mod negotiate;
mod request;
mod server_info;
mod small;
mod tls;
mod token_ops;
mod tree_ops;
mod upload;
mod wire_table;

pub use client::{ConnectError, HttpSyncClient, SyncApi};
pub use negotiate::Limits;

pub use config::SyncConfig;
pub use download::Verified;
pub use error::{Mapped, PlannerInput, SyncError};
pub use hash_ops::{HashCheckpoint, HashedFile};
pub use host::{
    BatchRecord, CheckpointStore, Credential, CredentialProvider, CredentialUnavailable,
    DownloadSink, FileIdentity, FileSource, MemoryCheckpointStore, PersistFailed, ProgressSink,
    StoreError, UploadCheckpoint, UploadState,
};
pub use request::{BoxFuture, Method, Request, RequestPath, Transport, WireRequest, WireResponse};
pub use small::{
    BatchOutcome, BatchReport, BatchRunError, BatchSink, SMALL_FILE_MAX, SkippedFile, SmallFile,
    SourceResolver, new_batch, takes_staged_path,
};
pub use upload::UploadRequest;
pub use wire_table::map_problem;
