//! Recording doubles of the 03 transfer ports.

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use common::{Calls, NOW, id, sorted};
use contracts::{
    BatchCommit, BatchEntryResult, BatchResult, BlobRef, BlockAck, ByteRange, ByteStream,
    CommitActor, CommitService, CommitServiceError, CommitUpload, Committed, Disposition, Download,
    DownloadError, DownloadOptions, DownloadService, DownloadStatus, PlaintextStream,
    ReservationError, ReservationSource, RevisionOrCurrent, Staged, StartUpload,
    StartUploadOutcome, Started, StreamError, UploadError, UploadService, UploadState, async_trait,
};
use futures_core::Stream;
use sc_ids::{FileHash, Hash32};
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

/// A stream with no chunks.
struct Empty;

impl Stream for Empty {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

fn empty_body() -> ByteStream {
    Box::pin(Empty)
}

fn committed() -> Committed {
    Committed {
        node_id: id(),
        revision_id: Uuid::from_u128(2),
        conflict_copy: None,
    }
}

fn download(status: DownloadStatus) -> Download {
    let body: PlaintextStream = Box::pin(Empty);
    Download {
        status,
        total_size: 0,
        range: None,
        etag: "blob".to_owned(),
        media_type: None,
        disposition: Disposition::Attachment,
        suggested_name: "a".to_owned(),
        body,
    }
}

fn actor() -> CommitActor {
    CommitActor {
        account_id: id(),
        device_label: "laptop".to_owned(),
    }
}

#[async_trait]
impl UploadService for Double {
    async fn start_upload(
        &self,
        _account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        self.0.record("start_upload");
        if request.content_hash.is_some() {
            return Ok(StartUploadOutcome::NothingToSend {
                revision: Uuid::from_u128(2),
            });
        }
        Ok(StartUploadOutcome::Started(Started {
            upload_id: id(),
            need_blocks: vec![0],
            block_size: 2 << 20,
            lane_limit: 4,
            expires_at: NOW,
        }))
    }

    async fn get_upload(
        &self,
        _account: Uuid,
        _upload_id: Uuid,
    ) -> Result<UploadState, UploadError> {
        self.0.record("get_upload");
        Ok(UploadState {
            need_blocks: Vec::new(),
            received_count: 1,
            expires_at: NOW,
        })
    }

    async fn put_block(
        &self,
        _account: Uuid,
        _upload_id: Uuid,
        _index: u32,
        _expected_hash: Hash32,
        _body: ByteStream,
        _length: u64,
    ) -> Result<BlockAck, UploadError> {
        self.0.record("put_block");
        Ok(BlockAck)
    }

    async fn abort_upload(&self, _account: Uuid, _upload_id: Uuid) -> Result<(), UploadError> {
        self.0.record("abort_upload");
        Ok(())
    }

    async fn stage_small(
        &self,
        _account: Uuid,
        staged_key: Uuid,
        _expected_hash: Option<Hash32>,
        _body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError> {
        self.0.record("stage_small");
        Ok(Staged {
            staged_key,
            size: i64::try_from(length).map_err(|e| UploadError::Unexpected(Box::new(e)))?,
        })
    }
}

#[async_trait]
impl CommitService for Double {
    async fn commit_upload(
        &self,
        _actor: CommitActor,
        _request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        self.0.record("commit_upload");
        Ok(committed())
    }

    async fn commit_batch(
        &self,
        _actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        self.0.record("commit_batch");
        Ok(BatchResult {
            per_entry: request
                .entries
                .iter()
                .map(|_| BatchEntryResult::Committed(committed()))
                .collect(),
        })
    }
}

#[async_trait]
impl DownloadService for Double {
    async fn open_revision_content(
        &self,
        _account: Uuid,
        _target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        self.0.record("open_revision_content");
        Ok(download(if options.range.is_some() {
            DownloadStatus::Partial
        } else {
            DownloadStatus::Full
        }))
    }

    async fn open_blob_content(
        &self,
        _account: Uuid,
        _blob: BlobRef,
        _media_type: Option<String>,
        _options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        self.0.record("open_blob_content");
        Ok(download(DownloadStatus::NotModified))
    }
}

#[async_trait]
impl ReservationSource for Double {
    async fn open_reserved_bytes(&self, _account: Uuid) -> Result<i64, ReservationError> {
        self.0.record("open_reserved_bytes");
        Ok(7)
    }
}

fn start() -> StartUpload {
    StartUpload {
        parent_id: id(),
        name: "a".to_owned(),
        declared_size: 10,
        client_key: "desk:1".to_owned(),
        content_hash: None,
        base_revision: None,
        client_meta: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_service_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let uploads: Arc<dyn UploadService> = Arc::new(Double(calls.clone()));

    let handle = tokio::spawn(async move {
        let account = id();
        let started = uploads.start_upload(account, start()).await?;
        uploads.get_upload(account, account).await?;
        uploads
            .put_block(
                account,
                account,
                0,
                Hash32::from_bytes([1; 32]),
                empty_body(),
                0,
            )
            .await?;
        uploads.abort_upload(account, account).await?;
        let staged = uploads
            .stage_small(account, Uuid::from_u128(4), None, empty_body(), 0)
            .await?;
        Ok::<_, UploadError>((started, staged))
    });
    let (started, staged) = handle.await.expect("task").expect("calls");

    assert!(matches!(started, StartUploadOutcome::Started(_)));
    assert_eq!(staged.size, 0);
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "start_upload",
            "get_upload",
            "put_block",
            "abort_upload",
            "stage_small"
        ])
    );
}

#[tokio::test]
async fn start_upload_with_a_known_hash_can_answer_nothing_to_send() {
    let uploads = Double::default();
    let mut request = start();
    request.content_hash = Some(FileHash::from_bytes([2; 32]));

    let outcome = uploads.start_upload(id(), request).await.expect("start");

    assert!(matches!(outcome, StartUploadOutcome::NothingToSend { .. }));
}

#[tokio::test]
async fn commit_service_double_records_both_methods() {
    let calls = Calls::default();
    let commits: Arc<dyn CommitService> = Arc::new(Double(calls.clone()));
    let upload = CommitUpload {
        upload_id: id(),
        parent_id: id(),
        name: "a".to_owned(),
        base_revision: None,
        blocklist: Vec::new(),
        client_mtime: None,
        client_meta: None,
        import_hint: false,
    };
    let batch = BatchCommit {
        commit_id: id(),
        entries: Vec::new(),
    };

    let single = commits
        .commit_upload(actor(), upload)
        .await
        .expect("commit");
    let many = commits.commit_batch(actor(), batch).await.expect("batch");

    assert_eq!(single.conflict_copy, None);
    assert!(many.per_entry.is_empty());
    assert_eq!(
        calls.sorted(),
        sorted(vec!["commit_upload", "commit_batch"])
    );
}

#[tokio::test]
async fn download_service_double_serves_by_revision_and_by_blob() {
    let calls = Calls::default();
    let downloads: Arc<dyn DownloadService> = Arc::new(Double(calls.clone()));
    let ranged = DownloadOptions {
        range: Some(ByteRange { start: 0, end: 9 }),
        ..DownloadOptions::default()
    };
    let blob = BlobRef {
        blob_id: id(),
        size: 10,
        wrapped_key: vec![1, 2, 3],
        master_key_id: 1,
    };

    let partial = downloads
        .open_revision_content(id(), RevisionOrCurrent::Current(id()), ranged)
        .await
        .expect("revision");
    let derived = downloads
        .open_blob_content(
            id(),
            blob,
            Some("image/jpeg".to_owned()),
            DownloadOptions::default(),
        )
        .await
        .expect("blob");

    assert_eq!(partial.status, DownloadStatus::Partial);
    assert_eq!(derived.status, DownloadStatus::NotModified);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["open_revision_content", "open_blob_content"])
    );
}

#[tokio::test]
async fn reservation_source_double_reports_open_reserved_bytes() {
    let source: Arc<dyn ReservationSource> = Arc::new(Double::default());

    let bytes = source.open_reserved_bytes(id()).await.expect("reserved");

    assert_eq!(bytes, 7);
}

#[test]
fn download_options_default_serves_only_current_revisions_without_conditions() {
    let options = DownloadOptions::default();

    assert!(!options.retained_ok);
    assert_eq!(
        (options.range, options.if_range, options.if_none_match),
        (None, None, None)
    );
}
