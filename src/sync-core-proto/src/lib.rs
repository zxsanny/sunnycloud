//! Sans-IO protocol types and rules of the sunnycloud client, shared by the native executor (`sync-core`)
//! and the browser Worker (`sync-core-wasm`).
//!
//! No tokio, HTTP, clock or OS randomness of its own: time and randomness enter as arguments. Hashing and
//! key derivation are `sc_ids`. Builds natively and for `wasm32-unknown-unknown`.

mod auth;
mod cursor;
mod digest_serde;
mod feed;
mod hashing;
mod keys;
mod page_check;
mod planner;
mod server_info;
mod transfer;
mod tree;
mod url;

pub use auth::{DeviceAuthorization, DevicePoll, SessionTokens};
pub use cursor::{BadCursor, Cursor};
pub use feed::{ChangeItem, ChangePage, ChangeState, ResyncReason};
pub use hashing::{ResumableFileHasher, ResumeHashState, hash_block};
pub use keys::{BatchIdError, check_batch_ids, client_key_for_mobile, reuse_or_new_key};
pub use page_check::{PageRule, PageViolation, check_page};
pub use planner::{Action, BlockResult, CommitResult, RetryPolicy, UploadPlanner};
pub use sc_ids::{BLOCK_SIZE, FileHash, FileHasher, Hash32, HashError, rollup};
pub use server_info::{ServerInfo, effective_lanes};
pub use transfer::{
    BatchCommit, BatchEntry, BatchResult, CommitOutcome, EntryResult, StartOutcome, UploadSession,
};
pub use tree::{
    ListingItem, ListingPage, NodeKind, NodeView, PageToken, RevisionRef, RevisionView,
};
pub use url::{ServerUrl, UrlError};

/// Client cap on parallel block uploads; the effective count is [`effective_lanes`].
pub const MAX_CLIENT_LANES: u32 = 4;

/// Maximum hashes in one `revisions:lookup` request.
pub const LOOKUP_CHUNK: usize = 1000;
