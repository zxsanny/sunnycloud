//! The fake grant server at the `SyncApi` boundary (tests.md unit tier): scripted device-grant answers, poll
//! instants read from the `ManualClock`, a sign-out answer, and a feed long-poll held until released.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use sc_clock::{Clock, ManualClock, MonotonicInstant};
use sync_core::{
    BoxFuture, DownloadSink, FileSource, HashCheckpoint, HashedFile, ProgressSink, SyncApi,
    SyncError, Verified,
};
use sync_core_proto::{
    BatchCommit, BatchResult, ChangePage, Cursor, DeviceAuthorization, DevicePoll, FileHash,
    Hash32, ListingPage, NodeView, PageToken, ResumeHashState, RevisionRef, RevisionView,
    SessionTokens, Staged,
};
use tokio::sync::Notify;
use uuid::Uuid;

pub(super) const USER_CODE: &str = "BCDF-GHJK";
pub(super) const VERIFICATION_URL: &str = "https://sync.example.test/device";

pub(super) struct FakeApi {
    clock: Arc<ManualClock>,
    grant: Result<DeviceAuthorization, SyncError>,
    polls: Mutex<VecDeque<Result<DevicePoll, SyncError>>>,
    granted_at: Mutex<Option<MonotonicInstant>>,
    poll_times: Mutex<Vec<MonotonicInstant>>,
    sign_out: Result<(), SyncError>,
    requests: AtomicUsize,
    feed_release: Notify,
}

impl FakeApi {
    pub(super) fn new(clock: Arc<ManualClock>, interval_secs: u64) -> Self {
        Self {
            clock,
            grant: Ok(DeviceAuthorization {
                device_code: "device-code-1".to_owned(),
                user_code: USER_CODE.to_owned(),
                verification_uri: VERIFICATION_URL.to_owned(),
                interval: interval_secs,
            }),
            polls: Mutex::new(VecDeque::new()),
            granted_at: Mutex::new(None),
            poll_times: Mutex::new(Vec::new()),
            sign_out: Ok(()),
            requests: AtomicUsize::new(0),
            feed_release: Notify::new(),
        }
    }

    pub(super) fn answering(self, polls: Vec<Result<DevicePoll, SyncError>>) -> Self {
        *self.polls.lock().expect("polls") = polls.into();
        self
    }

    pub(super) fn grant_failing(mut self, error: SyncError) -> Self {
        self.grant = Err(error);
        self
    }

    pub(super) fn sign_out_failing(mut self, error: SyncError) -> Self {
        self.sign_out = Err(error);
        self
    }

    pub(super) fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// Gaps between consecutive polls, and from the grant to the first poll.
    pub(super) fn poll_gaps(&self) -> Vec<Duration> {
        let times = self.poll_times.lock().expect("poll times");
        let mut previous = self
            .granted_at
            .lock()
            .expect("grant time")
            .expect("the grant was requested");
        times
            .iter()
            .map(|at| {
                let gap = at.saturating_duration_since(previous);
                previous = *at;
                gap
            })
            .collect()
    }

    /// Ends the held feed long-poll; it answers `Unauthorized` (the device was revoked meanwhile).
    pub(super) fn release_feed(&self) {
        self.feed_release.notify_one();
    }

    fn count(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) fn token(access: &str) -> DevicePoll {
    DevicePoll::Token(SessionTokens {
        access: access.to_owned(),
        refresh: None,
        expires_in: 0,
    })
}

fn unscripted<'a, T: Send + 'a>() -> BoxFuture<'a, Result<T, SyncError>> {
    Box::pin(async {
        Err(SyncError::Unexpected {
            code: "not scripted".to_owned(),
            request_id: None,
        })
    })
}

impl SyncApi for FakeApi {
    fn cancel(&self) {}

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>> {
        unscripted()
    }

    fn list_children<'a>(
        &'a self,
        _node: Uuid,
        _after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>> {
        unscripted()
    }

    fn get_revision(&self, _revision: Uuid) -> BoxFuture<'_, Result<RevisionView, SyncError>> {
        unscripted()
    }

    fn lookup_revisions<'a>(
        &'a self,
        _hashes: &'a [FileHash],
    ) -> BoxFuture<'a, Result<Vec<RevisionRef>, SyncError>> {
        unscripted()
    }

    fn create_folder<'a>(
        &'a self,
        _parent: Uuid,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        unscripted()
    }

    fn move_node<'a>(
        &'a self,
        _node: Uuid,
        _new_parent: Uuid,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        unscripted()
    }

    fn delete_node(
        &self,
        _node: Uuid,
        _base_revision: Option<Uuid>,
    ) -> BoxFuture<'_, Result<(), SyncError>> {
        unscripted()
    }

    /// An upload in flight: it never ends by itself.
    fn stage_small(
        &self,
        _staged_key: Uuid,
        _bytes: Bytes,
        _hash: Hash32,
    ) -> BoxFuture<'_, Result<Staged, SyncError>> {
        self.count();
        Box::pin(std::future::pending())
    }

    fn batch_commit(&self, _commit: BatchCommit) -> BoxFuture<'_, Result<BatchResult, SyncError>> {
        unscripted()
    }

    fn hash_file<'a>(
        &'a self,
        _source: Arc<dyn FileSource>,
        _resume: Option<ResumeHashState>,
        _progress: Arc<dyn ProgressSink>,
        _checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>> {
        unscripted()
    }

    fn download<'a>(
        &'a self,
        _revision: &'a RevisionRef,
        _sink: Arc<dyn DownloadSink>,
        _resume_from: u64,
    ) -> BoxFuture<'a, Result<Verified, SyncError>> {
        unscripted()
    }

    /// The feed long-poll is held until [`FakeApi::release_feed`].
    fn poll_changes(
        &self,
        _cursor: Cursor,
        _wait: Duration,
        _limit: Option<u32>,
    ) -> BoxFuture<'_, Result<ChangePage, SyncError>> {
        self.count();
        Box::pin(async {
            self.feed_release.notified().await;
            Err(SyncError::Unauthorized)
        })
    }

    fn head_cursor(&self) -> BoxFuture<'_, Result<Cursor, SyncError>> {
        unscripted()
    }

    fn device_grant_request<'a>(
        &'a self,
        _device_name: &'a str,
        _platform: &'a str,
    ) -> BoxFuture<'a, Result<DeviceAuthorization, SyncError>> {
        self.count();
        *self.granted_at.lock().expect("grant time") = Some(self.clock.monotonic());
        let grant = self.grant.clone();
        Box::pin(async move { grant })
    }

    fn device_grant_poll<'a>(
        &'a self,
        _device_code: &'a str,
    ) -> BoxFuture<'a, Result<DevicePoll, SyncError>> {
        self.count();
        self.poll_times
            .lock()
            .expect("poll times")
            .push(self.clock.monotonic());
        let answer = self
            .polls
            .lock()
            .expect("polls")
            .pop_front()
            .unwrap_or(Ok(DevicePoll::Pending));
        Box::pin(async move { answer })
    }

    fn exchange_mobile_code<'a>(
        &'a self,
        _code: &'a str,
        _code_verifier: &'a str,
    ) -> BoxFuture<'a, Result<SessionTokens, SyncError>> {
        unscripted()
    }

    fn refresh(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        unscripted()
    }

    fn sign_out(&self) -> BoxFuture<'_, Result<(), SyncError>> {
        self.count();
        let answer = self.sign_out.clone();
        Box::pin(async move { answer })
    }
}
