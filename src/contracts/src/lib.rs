//! Server-only port traits and their DTOs (ADR-015): every cross-component call goes through a trait here,
//! so no component crate imports another. Signatures only; the components implement them and `sunnycloud`
//! (12) wires them as `Arc<dyn Port>`.
//!
//! Contracts: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (registries and
//! lifecycle, 01 identity, 07 admin, 07a quota) and `part2_tree_transfer_feed.md` (02 tree and the
//! `CommitEffect` protocol, 03 transfer, 05 sync feed, the blob data-plane value types) and
//! `part3_blob_share_media_vault.md` (04 blobstore, 06 sharing, 08 media jobs, 20 vault).
//!
//! # Conventions
//!
//! - Every port is object safe and `Send + Sync`; methods are async unless marked sync. Implement a port with
//!   `#[contracts::async_trait]`.
//! - A method that joins the caller's transaction takes `tx: &mut DbTx`; every other method opens its own
//!   short transaction in the provider. A `DbTx` cannot be built outside `sc_db`:
//!
//! ```compile_fail,E0423
//! let _tx = sc_db::DbTx;
//! ```
//!
//! ```compile_fail,E0451
//! let _tx = sc_db::DbTx { depth: 0 };
//! ```
//!
//! - Each port has one error enum. Its typed variants are the contract's; `Unexpected` carries a tier-3 fault
//!   for the log. Providers convert these to `sc_error::BusinessError`; only 11 chooses an HTTP status.
//! - A DTO that carries a token prints a placeholder in `Debug` (NFT-SEC-16).

mod admin;
mod blob;
mod blobstore;
mod common;
mod feed;
mod identity;
mod media;
mod quota;
mod registries;
mod sharing;
mod transfer;
mod tree;
mod vault;

pub use async_trait::async_trait;

pub use admin::{
    AccountDeletionService, AccountService, AccountServiceError, AccountView, Actor, AdminError,
    AdminService, AdminUserView, DeletionAccepted, DeletionError, DevSeed, DevSeedError,
    HealthReport, IdentityView,
};
pub use blob::{BlobRef, ByteRange, ByteStream, PlaintextStream, StreamError};
pub use blobstore::{
    BlobCatalog, BlobCatalogError, BlobError, BlobHandle, BlobInventory, BlobPlan, BlobReader,
    BlobRegistry, BlobRegistryError, BlobWriter, ByteRate, Due, DurableToken, EpochMarker,
    EpochMarkerError, GcQueue, GcQueueError, GcReason, InventoryError, KeyError, Keyring,
    MasterKey, MasterKeyring, RegisterState, ReinstateOutcome, ScrubError, ScrubMode, ScrubRun,
    ScrubStatus, ScrubSummary, Scrubber, StepResult, WrittenBlob, WrittenOrPlanned,
};
pub use common::{Fault, Page};
pub use feed::{
    ChangeFeed, ChangeFeedError, ChangeItem, ChangePage, Cursor, EpochAdmin, EpochAdminError,
    FeedHorizon, FeedHorizonError, FeedNotifier, FeedStamp, FeedStampError, HeadReport, Marker,
    MarkerStatus, ResyncReason, Seq, TouchedNodes,
};
pub use identity::*;
pub use media::{
    AcceptOutcome, ClaimedJob, DerivedError, DerivedKind, DerivedOpener, FailDisposition,
    FailResult, JobCount, JobKind, JobStatus, Lane, LeaseExtended, MediaAdmin, MediaAdminError,
    MediaStats, OutputMeta, RequeueFilter, SkipOutcome, ThumbState, ThumbnailStateError,
    ThumbnailStateSource, WorkerAuthError, WorkerAuthenticator, WorkerIdentity, WorkerInput,
    WorkerJobError, WorkerJobs, WorkerStatus,
};
pub use quota::{
    Drift, QuotaAdmin, QuotaAdminError, QuotaLedger, QuotaLedgerError, QuotaView, Reserved,
};
pub use registries::{
    AccountLifecycleHook, BlobCleanupError, BlobOwnerCleanup, CheckFuture, CycleStats, DoctorCheck,
    DoctorContext, DoctorTag, EraseProgress, Finding, FindingLevel, HealthContributor, HealthItem,
    HealthStatus, Janitor, JanitorError, LifecycleError, ReadinessCheck, ReadinessItem,
    ReadinessStatus,
};
pub use sharing::{
    AccessGrant, CreatedShare, PublicShareError, PublicShareService, ShareContent, ShareError,
    ShareInfo, ShareOpen, ShareService,
};
pub use transfer::{
    BatchCommit, BatchEntry, BatchEntryResult, BatchResult, BlockAck, CommitService,
    CommitServiceError, CommitUpload, Committed, Disposition, Download, DownloadError,
    DownloadOptions, DownloadService, DownloadStatus, EntryError, ReservationError,
    ReservationSource, RevisionOrCurrent, Staged, StartUpload, StartUploadOutcome, Started,
    UploadError, UploadService, UploadState,
};
pub use tree::{
    Begin, CommitActor, CommitEffect, EffectError, EntryFailure, EntryOutcome, EntryResult,
    ListingPage, LiveFile, LiveFileLookup, LiveUsageError, LiveUsageSource, MaintenanceError,
    NewRevision, NodeKind, NodeOrPath, NodeRef, NodeSelector, NodeState, NodeView, Placement,
    PublishEntry, PublishOutcome, PublishResult, PublishedEntry, RestoreError, RestoreOutcome,
    RevisionMatch, RevisionRef, RevisionView, StoredResponse, Target, TreeCommit, TreeCommitError,
    TreeMaintenance, TreeReader, TreeReaderError, TreeRestore, TreeService, TreeServiceError,
};
pub use vault::{CreatedVault, ETag, VaultError, VaultHeader, VaultService};
