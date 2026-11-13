//! `HttpSyncClient`, the one production [`SyncApi`], and `connect` (`sync_api.md` construction and negotiation).

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use sc_clock::{Clock, SystemClock};
use sync_core_proto::{
    BatchCommit, BatchResult, ChangePage, CommitOutcome, Cursor, DeviceAuthorization, DevicePoll,
    FileHash, Hash32, ListingPage, NodeView, PageToken, ResumeHashState, RetryPolicy, RevisionRef,
    RevisionView, ServerInfo, ServerUrl, SessionTokens, Staged, UrlError,
};
use thiserror::Error;
use uuid::Uuid;

use crate::config::SyncConfig;
use crate::download::{self, Verified};
use crate::error::SyncError;
use crate::feed_ops::{self, Feed};
use crate::hash_ops::{HashCheckpoint, HashedFile, Hashing};
use crate::host::{CheckpointStore, CredentialProvider, DownloadSink, FileSource, ProgressSink};
use crate::http_transport::HttpTransport;
use crate::negotiate::{Limits, identity, negotiate};
use crate::request::{BoxFuture, RequestPath, jitter};
use crate::server_info::fetch_server_info;
use crate::small::{
    self, BatchRunError, BatchSink, Runner, SkippedFile, SmallFile, SourceResolver,
};
use crate::upload::{UploadRequest, Uploader};
use crate::{token_ops, tree_ops};

/// Why `connect` gave no client. Messages are fixed strings: nothing from the server reaches `Display`.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ConnectError {
    /// `sync.server_url` is `http://` and the build does not have `allow-plain-http`.
    #[error("plain http is not allowed")]
    InsecureScheme,
    /// No usable answer from the server: no connection, a failed TLS 1.3 / `h2` handshake, a timeout, or a refusal to serve.
    #[error("server unreachable")]
    Unreachable,
    /// `message` is meant for the user. `update_url` is `None` when the client decided itself: `server-info` carries none.
    #[error("client upgrade required")]
    UpgradeRequired {
        message: String,
        update_url: Option<String>,
    },
    /// The server's `server-info` is outside what this client accepts; `what` names the field.
    #[error("protocol mismatch ({what})")]
    ProtocolMismatch { what: &'static str },
    #[error("unauthorized")]
    Unauthorized,
    /// A local setting the client cannot use (a malformed server URL, client family or client version).
    #[error("invalid configuration ({field})")]
    InvalidConfig { field: &'static str },
}

/// The typed API hosts code against. Operations are added to this trait by their own tasks (`sync_api.md` § Operations).
pub trait SyncApi: Send + Sync {
    /// Ends every operation in flight with `Cancelled`; nothing is sent afterwards.
    fn cancel(&self);

    /// `GET /api/v1/nodes/root`.
    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>>;

    /// One page of a folder, files with their `content_hash`; `after` is the previous page's `next`,
    /// `None` for the first page. The last page has `next = None`. Errors: `NotFound`, `NotAFolder`.
    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>>;

    /// Error: `NotFound`.
    fn get_revision(&self, revision: Uuid) -> BoxFuture<'_, Result<RevisionView, SyncError>>;

    /// Current revisions of live files with these content hashes, in requests of at most `LOOKUP_CHUNK`;
    /// unknown hashes return nothing. For the reinstall rebuild only, never to skip upload bytes.
    fn lookup_revisions<'a>(
        &'a self,
        hashes: &'a [FileHash],
    ) -> BoxFuture<'a, Result<Vec<RevisionRef>, SyncError>>;

    /// Sent once, never retried. Errors: `NameConflict`, `InvalidName`, `ParentNotFound`, `ParentGone`, `NotAFolder`.
    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>>;

    /// Moves and/or renames; carries no base revision, so it never conflicts with an edit.
    /// Errors: `NameConflict`, `InvalidName`, `Cycle`, `ParentNotFound`, `ParentGone`, `NotAFolder`, `NotFound`.
    fn move_node<'a>(
        &'a self,
        node: Uuid,
        new_parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>>;

    /// `base_revision` is the file's revision the caller last saw; `None` for a folder.
    /// Errors: `EditedSince` (a file changed on the server meanwhile), `NotFound`.
    fn delete_node(
        &self,
        node: Uuid,
        base_revision: Option<Uuid>,
    ) -> BoxFuture<'_, Result<(), SyncError>>;

    /// Stages one file of at most `small_file_max` bytes in one request, idempotent by `staged_key`; `hash` is
    /// the BLAKE3 of `bytes` (sent as `X-Blake3`). Errors: `QuotaExceeded`, `StagedCapReached`, `FreeSpaceFloor`.
    fn stage_small(
        &self,
        staged_key: Uuid,
        bytes: Bytes,
        hash: Hash32,
    ) -> BoxFuture<'_, Result<Staged, SyncError>>;

    /// Idempotent by `commit_id`: a repeated id with the same entries returns the stored response.
    /// Errors: `BatchTooLarge`, `CommitIdReused`.
    fn batch_commit(&self, commit: BatchCommit) -> BoxFuture<'_, Result<BatchResult, SyncError>>;

    /// Hashes the host file block by block. With `resume`, only the blocks after `resume.blocks_done` are read.
    /// The hash state is stored through `checkpoint` every `sync.hash_checkpoint_blocks` blocks (the last block
    /// excepted) and the CPU use is held to `sync.hash_cpu_share`. `progress` counts hashed bytes.
    /// Errors: `SourceChanged` (size or modification time differ at the end, the file is shorter than the
    /// resume state or than its length), `Io` (the source; the last stored state stays valid).
    fn hash_file<'a>(
        &'a self,
        source: Arc<dyn FileSource>,
        resume: Option<ResumeHashState>,
        progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>>;

    /// Fetches the revision into `sink` by ranged requests of one block and continues at the block that holds
    /// `resume_from`. The sink gets verified blocks only, in order; each block is checked against the
    /// revision's blocklist before `write_verified` and the whole file against `content_hash` before
    /// `finish(true)`; after any failure past the revision lookup it gets `finish(false)`. Errors: `NotFound`,
    /// `BlobUnavailable`, `HashMismatch{block}` (the first bad block), `Io` (the sink),
    /// `ProtocolViolation` (an answer that is not the requested range, a short body, an inconsistent revision).
    fn download<'a>(
        &'a self,
        revision: &'a RevisionRef,
        sink: Arc<dyn DownloadSink>,
        resume_from: u64,
    ) -> BoxFuture<'a, Result<Verified, SyncError>>;

    /// One page of the change feed after `cursor`. The server holds an empty read for `wait`, clamped to its
    /// `longpoll_max_wait`; the call gives up after the clamped wait plus `sync.longpoll_client_margin` with
    /// `Transport(Timeout)`. `has_more` is returned as is: the caller polls again at once, the client adds no
    /// delay. After a failure the next call first rests for the backoff delay (`sync.retry_base` doubling up
    /// to `sync.retry_max`, or `Retry-After`, with jitter). Every page is checked with P1..P6 (`check_page`)
    /// before it is returned. Errors: `ResyncRequired{reason}` (410, or 422 `invalid_cursor` as `InvalidCursor`:
    /// read `head_cursor`, then list), `ProtocolViolation{detail}` naming the broken rule. The core never
    /// stores the cursor; the host persists it after applying the page.
    fn poll_changes(
        &self,
        cursor: Cursor,
        wait: Duration,
        limit: Option<u32>,
    ) -> BoxFuture<'_, Result<ChangePage, SyncError>>;

    /// The account's current cursor: the first step of a resync, before any listing.
    fn head_cursor(&self) -> BoxFuture<'_, Result<Cursor, SyncError>>;

    /// Starts a desktop sign-in: the user approves `user_code` elsewhere while the host polls with `device_code`.
    /// Sent without a credential.
    fn device_grant_request<'a>(
        &'a self,
        device_name: &'a str,
        platform: &'a str,
    ) -> BoxFuture<'a, Result<DeviceAuthorization, SyncError>>;

    /// One poll; the host owns the interval and the slow-down. `Token` holds the device token, which the host
    /// stores. Sent without a credential.
    fn device_grant_poll<'a>(
        &'a self,
        device_code: &'a str,
    ) -> BoxFuture<'a, Result<DevicePoll, SyncError>>;

    /// Exchanges the one-time code of a mobile browser sign-in; the host stores the returned tokens.
    /// Errors: `InvalidCode` (reused, expired or wrong verifier). Sent without a credential.
    fn exchange_mobile_code<'a>(
        &'a self,
        code: &'a str,
        code_verifier: &'a str,
    ) -> BoxFuture<'a, Result<SessionTokens, SyncError>>;

    /// Rotates a mobile session's tokens now, joined with a rotation already running. The new pair reaches the
    /// host only through `CredentialProvider::on_rotated`, which returned `Ok` before this does. A 401 on any
    /// other call does the same by itself; this is for a host that refreshes ahead of expiry.
    /// `Unauthorized` (revoked, expired, reused, or the pair could not be stored) stops all work until a new
    /// sign-in.
    fn refresh(&self) -> BoxFuture<'_, Result<(), SyncError>>;

    /// Revokes this device token or session on the server; the host clears its copy afterwards.
    fn sign_out(&self) -> BoxFuture<'_, Result<(), SyncError>>;
}

pub struct HttpSyncClient {
    path: Arc<RequestPath>,
    limits: Limits,
    clock: Arc<dyn Clock>,
    hashing: Hashing,
    feed: Feed,
    batch_max_bytes_in_flight: u64,
    policy: RetryPolicy,
}

impl HttpSyncClient {
    /// Checks the server address, reads and range-checks `server-info`, and returns the client with the
    /// answer as sent. The negotiated limits are [`limits`](Self::limits).
    pub async fn connect(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        Self::establish(config, credentials, Vec::new()).await
    }

    /// `connect` that also trusts `roots`: the scripted fake server signs with a CA generated per test run.
    #[cfg(feature = "test-support")]
    pub async fn connect_with_roots(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
        roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        Self::establish(config, credentials, roots).await
    }

    /// The request path every operation sends through.
    #[cfg(feature = "test-support")]
    pub fn request_path(&self) -> &RequestPath {
        &self.path
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Uploads a hashed file by blocks: `start_upload` with the stored `client_key` (see
    /// [`UploadRequest::client_key`]), each needed block `PUT` with its BLAKE3 on at most
    /// `min(4, ServerInfo.max_lanes, UploadSession.lane_limit)` lanes, done only on 204, then one commit. The
    /// checkpoint is saved before the first block and cleared after an outcome; a resume re-reads the session
    /// and sends only `need_blocks`. Before the commit the source's identity is checked again; after
    /// `blocklist_mismatch` the file is hashed again and the session restarted with the same key. A lost commit
    /// answer is recovered by one `start_upload` with the identical request (`sync_api.md` § Lost-commit
    /// recovery). `progress` counts the bytes of acknowledged blocks.
    /// Outcomes: `Published`, `ConflictCopy` (the name is the server's; never retried under another name),
    /// `NothingToSend` (0 block bytes). Errors: `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`,
    /// `FreeSpaceFloor`, `InvalidName`, `ParentNotFound`, `ParentGone`, `NotAFolder`, `BlocklistMismatch`
    /// (restarts used up), `SourceChanged`, `Io` (the source); a block given up after
    /// `sync.max_attempts_per_block` sends reports its last failure. The checkpoint is kept on every error.
    pub async fn upload(
        &self,
        request: UploadRequest,
        checkpoints: Arc<dyn CheckpointStore>,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<CommitOutcome, SyncError> {
        let uploader = Uploader {
            path: &self.path,
            hashing: &self.hashing,
            lanes: self.limits.lanes,
            policy: self.policy,
            jitter,
        };
        uploader.upload(request, checkpoints, progress).await
    }

    /// Uploads files of at most `SMALL_FILE_MAX` through the staged path in batches. Each batch record is saved
    /// to `store` before its first file is staged and cleared after `sink` stored the results. Staging runs in
    /// parallel; a batch stays within `batch_max_entries`, within `sync.batch_max_bytes_in_flight` and within
    /// half of `max_staged_per_account`. Files that cannot be sent are returned, not retried.
    /// Errors: the common set, `QuotaExceeded`, `FreeSpaceFloor`, `StagedCapReached` when nothing at all
    /// could be staged, and `BatchRunError::Store`. A dropped or failed run is continued by
    /// [`resume_batches`](Self::resume_batches).
    pub async fn upload_small(
        &self,
        files: Vec<SmallFile>,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
    ) -> Result<Vec<SkippedFile>, BatchRunError> {
        self.small_runner(store, sink).upload(files).await
    }

    /// Replays the batches `store` still holds: same `commit_id`, same entries. `resolve` finds the source
    /// of each entry.
    pub async fn resume_batches(
        &self,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
        resolve: SourceResolver<'_>,
    ) -> Result<Vec<SkippedFile>, BatchRunError> {
        self.small_runner(store, sink).resume(resolve).await
    }

    fn small_runner(&self, store: Arc<dyn CheckpointStore>, sink: Arc<dyn BatchSink>) -> Runner {
        Runner::new(
            Arc::clone(&self.path),
            self.limits,
            self.batch_max_bytes_in_flight,
            Arc::clone(&self.clock),
            store,
            sink,
        )
    }

    async fn establish(
        config: SyncConfig,
        credentials: Arc<dyn CredentialProvider>,
        extra_roots: Vec<rustls::pki_types::CertificateDer<'static>>,
    ) -> Result<(Self, ServerInfo), ConnectError> {
        let url = ServerUrl::parse(&config.server_url, cfg!(feature = "allow-plain-http"))
            .map_err(|error| match error {
                UrlError::InsecureScheme => ConnectError::InsecureScheme,
                UrlError::Invalid => ConnectError::InvalidConfig {
                    field: "server_url",
                },
            })?;
        let (header, own) = identity(&config)?;
        let transport =
            HttpTransport::new(&config, &url, &header, extra_roots).map_err(|error| {
                tracing::error!(%error, "http client setup failed");
                ConnectError::Unreachable
            })?;
        let info = fetch_server_info(&transport).await?;
        let limits = negotiate(&info, &config, own)?;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let feed = Feed::new(Arc::clone(&clock), &config, &info);
        let path = RequestPath::new(
            Arc::new(transport),
            credentials,
            Arc::clone(&clock),
            config.retry_policy(),
        );
        Ok((
            Self {
                path: Arc::new(path),
                limits,
                hashing: Hashing::new(Arc::clone(&clock), &config),
                feed,
                clock,
                batch_max_bytes_in_flight: config.batch_max_bytes_in_flight,
                policy: config.retry_policy(),
            },
            info,
        ))
    }
}

impl SyncApi for HttpSyncClient {
    fn cancel(&self) {
        self.path.cancel();
    }

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::root_node(&self.path))
    }

    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>> {
        Box::pin(tree_ops::list_children(&self.path, node, after))
    }

    fn get_revision(&self, revision: Uuid) -> BoxFuture<'_, Result<RevisionView, SyncError>> {
        Box::pin(tree_ops::get_revision(&self.path, revision))
    }

    fn lookup_revisions<'a>(
        &'a self,
        hashes: &'a [FileHash],
    ) -> BoxFuture<'a, Result<Vec<RevisionRef>, SyncError>> {
        Box::pin(tree_ops::lookup_revisions(&self.path, hashes))
    }

    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::create_folder(&self.path, parent, name))
    }

    fn move_node<'a>(
        &'a self,
        node: Uuid,
        new_parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        Box::pin(tree_ops::move_node(&self.path, node, new_parent, name))
    }

    fn delete_node(
        &self,
        node: Uuid,
        base_revision: Option<Uuid>,
    ) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(tree_ops::delete_node(&self.path, node, base_revision))
    }

    fn stage_small(
        &self,
        staged_key: Uuid,
        bytes: Bytes,
        hash: Hash32,
    ) -> BoxFuture<'_, Result<Staged, SyncError>> {
        Box::pin(async move { small::stage_small(&self.path, staged_key, bytes, &hash).await })
    }

    fn batch_commit(&self, commit: BatchCommit) -> BoxFuture<'_, Result<BatchResult, SyncError>> {
        Box::pin(async move { small::batch_commit(&self.path, &commit).await })
    }

    fn hash_file<'a>(
        &'a self,
        source: Arc<dyn FileSource>,
        resume: Option<ResumeHashState>,
        progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>> {
        Box::pin(
            self.hashing
                .hash_file(&self.path, source, resume, progress, checkpoint),
        )
    }

    fn download<'a>(
        &'a self,
        revision: &'a RevisionRef,
        sink: Arc<dyn DownloadSink>,
        resume_from: u64,
    ) -> BoxFuture<'a, Result<Verified, SyncError>> {
        Box::pin(download::download(&self.path, revision, sink, resume_from))
    }

    fn poll_changes(
        &self,
        cursor: Cursor,
        wait: Duration,
        limit: Option<u32>,
    ) -> BoxFuture<'_, Result<ChangePage, SyncError>> {
        Box::pin(self.feed.poll_changes(&self.path, cursor, wait, limit))
    }

    fn head_cursor(&self) -> BoxFuture<'_, Result<Cursor, SyncError>> {
        Box::pin(feed_ops::head_cursor(&self.path))
    }

    fn device_grant_request<'a>(
        &'a self,
        device_name: &'a str,
        platform: &'a str,
    ) -> BoxFuture<'a, Result<DeviceAuthorization, SyncError>> {
        Box::pin(token_ops::device_grant_request(
            &self.path,
            device_name,
            platform,
        ))
    }

    fn device_grant_poll<'a>(
        &'a self,
        device_code: &'a str,
    ) -> BoxFuture<'a, Result<DevicePoll, SyncError>> {
        Box::pin(token_ops::device_grant_poll(&self.path, device_code))
    }

    fn exchange_mobile_code<'a>(
        &'a self,
        code: &'a str,
        code_verifier: &'a str,
    ) -> BoxFuture<'a, Result<SessionTokens, SyncError>> {
        Box::pin(token_ops::exchange_mobile_code(
            &self.path,
            code,
            code_verifier,
        ))
    }

    fn refresh(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(self.path.refresh_now())
    }

    fn sign_out(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        Box::pin(token_ops::sign_out(&self.path))
    }
}
