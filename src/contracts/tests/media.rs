//! Recording doubles of the 08 media-job ports (AC-2) and the uniform worker authentication failure (AC-3).

mod common;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use common::{Calls, fault, id, sorted};
use contracts::{
    AcceptOutcome, BlobRef, ByteStream, ClaimedJob, DerivedBlob, DerivedError, DerivedKind,
    DerivedOpener, FailDisposition, FailResult, JobCount, JobKind, JobStatus, Lane, LeaseExtended,
    LiveFile, MediaAdmin, MediaAdminError, MediaStats, NodeKind, OutputMeta, RequeueFilter,
    SkipOutcome, StreamError, ThumbState, ThumbnailStateError, ThumbnailStateSource,
    WorkerAuthError, WorkerAuthenticator, WorkerIdentity, WorkerInput, WorkerJobError, WorkerJobs,
    WorkerStatus, async_trait,
};
use futures_core::Stream;
use sc_ids::FileHash;
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

struct Empty;

impl Stream for Empty {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

fn hash() -> FileHash {
    FileHash::from_bytes([3; 32])
}

fn blob() -> BlobRef {
    BlobRef {
        blob_id: id(),
        size: 10,
        wrapped_key: vec![1],
        master_key_id: 1,
    }
}

fn derived_blob() -> DerivedBlob {
    DerivedBlob {
        blob: blob(),
        media_type: "image/webp".to_owned(),
    }
}

fn meta() -> OutputMeta {
    OutputMeta {
        media_type: "image/webp".to_owned(),
        width: Some(320),
        height: Some(240),
        duration_ms: None,
    }
}

#[async_trait]
impl ThumbnailStateSource for Double {
    async fn states(
        &self,
        _account: Uuid,
        hashes: &[FileHash],
    ) -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError> {
        self.0.record("states");
        Ok(hashes.iter().map(|h| (*h, ThumbState::Pending)).collect())
    }
}

#[async_trait]
impl DerivedOpener for Double {
    async fn open_derived(
        &self,
        _account: Uuid,
        _node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        self.0.record("open_derived");
        match kind {
            DerivedKind::Thumb => Ok(derived_blob()),
            DerivedKind::Preview => Err(DerivedError::NotReady),
            DerivedKind::Transcode => Err(DerivedError::NotFound),
        }
    }

    async fn open_derived_for(
        &self,
        _account: Uuid,
        _file: &LiveFile,
        _kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        self.0.record("open_derived_for");
        Ok(derived_blob())
    }
}

#[async_trait]
impl WorkerAuthenticator for Double {
    async fn authenticate(&self, bearer: &str) -> Result<WorkerIdentity, WorkerAuthError> {
        self.0.record("authenticate");
        if bearer == "secret" {
            Ok(WorkerIdentity)
        } else {
            Err(WorkerAuthError::Forbidden)
        }
    }
}

#[async_trait]
impl WorkerJobs for Double {
    async fn claim(
        &self,
        lane: Lane,
        _worker_id: &str,
    ) -> Result<Option<ClaimedJob>, WorkerJobError> {
        self.0.record("claim");
        Ok(match lane {
            Lane::Image => Some(ClaimedJob {
                job_id: id(),
                kind: JobKind::Stills,
                content_hash: hash(),
                params_version: 1,
                attempt: 1,
                lease_ms: 60_000,
            }),
            Lane::Video => None,
        })
    }

    async fn extend_lease(
        &self,
        _job_id: Uuid,
        _worker_id: &str,
    ) -> Result<LeaseExtended, WorkerJobError> {
        self.0.record("extend_lease");
        Ok(LeaseExtended { lease_ms: 60_000 })
    }

    async fn release(&self, _job_id: Uuid, _worker_id: &str) -> Result<(), WorkerJobError> {
        self.0.record("release");
        Err(WorkerJobError::LeaseLost)
    }

    async fn complete_skipped(
        &self,
        _job_id: Uuid,
        _worker_id: &str,
        _outcome: SkipOutcome,
    ) -> Result<(), WorkerJobError> {
        self.0.record("complete_skipped");
        Ok(())
    }

    async fn fail(
        &self,
        _job_id: Uuid,
        _worker_id: &str,
        _code: &str,
        disposition: FailDisposition,
    ) -> Result<FailResult, WorkerJobError> {
        self.0.record("fail");
        Ok(match disposition {
            FailDisposition::Retry => FailResult::Requeued,
            FailDisposition::Permanent => FailResult::Dead,
        })
    }

    async fn open_input(
        &self,
        _job_id: Uuid,
        _worker_id: &str,
    ) -> Result<WorkerInput, WorkerJobError> {
        self.0.record("open_input");
        Ok(WorkerInput {
            length: 0,
            body: Box::pin(Empty),
        })
    }

    async fn accept_output(
        &self,
        _job_id: Uuid,
        _worker_id: &str,
        _kind: DerivedKind,
        _meta: OutputMeta,
        _body: ByteStream,
        length: u64,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        self.0.record("accept_output");
        if length > 1024 {
            return Err(WorkerJobError::OutputTooLarge);
        }
        Ok(AcceptOutcome::Accepted)
    }

    async fn record_status(
        &self,
        _worker_id: &str,
        _status: WorkerStatus,
    ) -> Result<(), WorkerJobError> {
        self.0.record("record_status");
        Ok(())
    }
}

#[async_trait]
impl MediaAdmin for Double {
    async fn requeue_dead(&self, filter: RequeueFilter) -> Result<u64, MediaAdminError> {
        self.0.record("requeue_dead");
        Ok(if filter.account.is_some() { 4 } else { 9 })
    }

    async fn stats(&self) -> Result<MediaStats, MediaAdminError> {
        self.0.record("stats");
        Ok(MediaStats {
            counts: vec![JobCount {
                lane: Lane::Image,
                status: JobStatus::Queued,
                count: 2,
            }],
        })
    }
}

#[tokio::test]
async fn thumbnail_state_source_double_answers_one_state_per_hash() {
    // Arrange
    let calls = Calls::default();
    let source: Arc<dyn ThumbnailStateSource> = Arc::new(Double(calls.clone()));

    // Act
    let states = source.states(id(), &[hash()]).await.expect("states");

    // Assert
    assert_eq!(states.get(&hash()), Some(&ThumbState::Pending));
    assert_eq!(calls.sorted(), vec!["states"]);
}

#[tokio::test]
async fn derived_opener_double_separates_not_ready_from_not_found() {
    // Arrange
    let calls = Calls::default();
    let opener: Arc<dyn DerivedOpener> = Arc::new(Double(calls.clone()));
    let file = LiveFile {
        kind: NodeKind::File,
        revision: None,
        blob_ref: None,
    };

    // Act
    let thumb = opener.open_derived(id(), id(), DerivedKind::Thumb).await;
    let preview = opener.open_derived(id(), id(), DerivedKind::Preview).await;
    let transcode = opener
        .open_derived(id(), id(), DerivedKind::Transcode)
        .await;
    let shared = opener
        .open_derived_for(id(), &file, DerivedKind::Thumb)
        .await;

    // Assert
    assert!(thumb.is_ok());
    assert!(matches!(preview, Err(DerivedError::NotReady)));
    assert!(matches!(transcode, Err(DerivedError::NotFound)));
    assert!(shared.is_ok());
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "open_derived",
            "open_derived",
            "open_derived",
            "open_derived_for"
        ])
    );
}

#[tokio::test]
async fn worker_authenticator_double_accepts_the_secret_and_refuses_every_other_bearer() {
    // Arrange
    let calls = Calls::default();
    let auth: Arc<dyn WorkerAuthenticator> = Arc::new(Double(calls.clone()));

    // Act
    let accepted = auth.authenticate("secret").await;
    let empty = auth.authenticate("").await;
    let user_session = auth.authenticate("a-user-session-token").await;

    // Assert
    assert_eq!(accepted.expect("worker"), WorkerIdentity);
    assert!(matches!(empty, Err(WorkerAuthError::Forbidden)));
    assert!(matches!(user_session, Err(WorkerAuthError::Forbidden)));
    assert_eq!(calls.sorted().len(), 3);
}

#[test]
fn worker_auth_error_has_exactly_one_variant() {
    // Arrange
    let error = WorkerAuthError::Forbidden;

    // Act
    let WorkerAuthError::Forbidden = error;

    // Assert
    assert_eq!(error.to_string(), "forbidden");
}

#[tokio::test]
async fn worker_jobs_double_records_every_method() {
    // Arrange
    let calls = Calls::default();
    let jobs: Arc<dyn WorkerJobs> = Arc::new(Double(calls.clone()));
    let body: ByteStream = Box::pin(Empty);

    // Act
    let image = jobs.claim(Lane::Image, "w1").await.expect("claim");
    let video = jobs.claim(Lane::Video, "w1").await.expect("claim");
    let lease = jobs.extend_lease(id(), "w1").await.expect("lease");
    let released = jobs.release(id(), "w1").await;
    jobs.complete_skipped(id(), "w1", SkipOutcome::Obsolete)
        .await
        .expect("skipped");
    let retried = jobs
        .fail(id(), "w1", "decode", FailDisposition::Retry)
        .await
        .expect("fail");
    let input = jobs.open_input(id(), "w1").await.expect("input");
    let accepted = jobs
        .accept_output(id(), "w1", DerivedKind::Thumb, meta(), body, 0)
        .await
        .expect("output");
    jobs.record_status(
        "w1",
        WorkerStatus {
            decoder_ok: true,
            image_age_days: 3,
        },
    )
    .await
    .expect("status");

    // Assert
    assert_eq!(image.map(|j| j.kind), Some(JobKind::Stills));
    assert!(video.is_none());
    assert_eq!(lease.lease_ms, 60_000);
    assert!(matches!(released, Err(WorkerJobError::LeaseLost)));
    assert_eq!(retried, FailResult::Requeued);
    assert_eq!(input.length, 0);
    assert_eq!(accepted, AcceptOutcome::Accepted);
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "claim",
            "claim",
            "extend_lease",
            "release",
            "complete_skipped",
            "fail",
            "open_input",
            "accept_output",
            "record_status"
        ])
    );
}

#[tokio::test]
async fn worker_jobs_accept_output_over_the_cap_is_too_large() {
    // Arrange
    let jobs: Arc<dyn WorkerJobs> = Arc::new(Double::default());
    let body: ByteStream = Box::pin(Empty);

    // Act
    let outcome = jobs
        .accept_output(id(), "w1", DerivedKind::Preview, meta(), body, 4096)
        .await;

    // Assert
    assert!(matches!(outcome, Err(WorkerJobError::OutputTooLarge)));
}

#[tokio::test]
async fn media_admin_double_requeues_by_filter_and_reports_stats() {
    // Arrange
    let calls = Calls::default();
    let admin: Arc<dyn MediaAdmin> = Arc::new(Double(calls.clone()));
    let one_account = RequeueFilter {
        account: Some(id()),
        kind: Some(JobKind::Poster),
    };

    // Act
    let filtered = admin.requeue_dead(one_account).await.expect("requeue");
    let all = admin
        .requeue_dead(RequeueFilter::default())
        .await
        .expect("requeue");
    let stats = admin.stats().await.expect("stats");

    // Assert
    assert_eq!((filtered, all), (4, 9));
    assert_eq!(stats.counts.len(), 1);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["requeue_dead", "requeue_dead", "stats"])
    );
}

#[test]
fn the_media_errors_other_than_authentication_carry_the_tier_three_variant() {
    // Arrange
    let states = ThumbnailStateError::Unexpected(fault());
    let derived = DerivedError::Unexpected(fault());
    let admin = MediaAdminError::Unexpected(fault());
    let jobs = WorkerJobError::Storage(fault());

    // Act
    let ThumbnailStateError::Unexpected(_) = states;
    let MediaAdminError::Unexpected(_) = admin;

    // Assert
    assert!(matches!(derived, DerivedError::Unexpected(_)));
    assert!(matches!(jobs, WorkerJobError::Storage(_)));
}
