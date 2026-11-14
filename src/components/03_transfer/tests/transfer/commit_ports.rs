//! Thin decorators of the commit's real ports: the 04 reader counts its calls (AC-1: one head read, no full
//! read) and marks each one `blob_reader` in the statement log (AZ-3757 AC-69; a no-op while no log is
//! installed), and 02's `publish` can be paused once before it delegates, so a test removes the session between
//! validation and the commit transaction (AC-5). The real 02 service behind it serves the tree mutations of the
//! moved-parent cases (AZ-3757 AC-70).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use contracts::{
    BlobError, BlobReader, ByteRange, CommitActor, CommitEffect, PlaintextStream, PublishEntry,
    PublishResult, TreeCommit, TreeCommitError, async_trait,
};
use sc_clock::Timestamp;
use sc_testdb::statements::mark;
use sunnycloud_blobstore::FileBlobReader;
use sunnycloud_tree::PgTreeService;
use tokio::sync::oneshot;
use uuid::Uuid;

/// The real reader; `opens` counts full or range reads, `heads` head reads, `largest_head` the biggest
/// `max_bytes` asked for.
pub struct CountingReader {
    inner: FileBlobReader,
    opens: AtomicUsize,
    heads: AtomicUsize,
    largest_head: AtomicU32,
}

impl CountingReader {
    pub fn new(inner: FileBlobReader) -> Self {
        CountingReader {
            inner,
            opens: AtomicUsize::new(0),
            heads: AtomicUsize::new(0),
            largest_head: AtomicU32::new(0),
        }
    }

    /// `(opens, heads, largest head)` since the last reset.
    pub fn counts(&self) -> (usize, usize, u32) {
        (
            self.opens.load(Ordering::SeqCst),
            self.heads.load(Ordering::SeqCst),
            self.largest_head.load(Ordering::SeqCst),
        )
    }

    pub fn reset(&self) {
        self.opens.store(0, Ordering::SeqCst);
        self.heads.store(0, Ordering::SeqCst);
        self.largest_head.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl BlobReader for CountingReader {
    async fn open(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        range: Option<ByteRange>,
    ) -> Result<PlaintextStream, BlobError> {
        mark("blob_reader");
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.inner
            .open(blob_id, wrapped_key, master_key_id, plaintext_size, range)
            .await
    }

    async fn read_head(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        max_bytes: u32,
    ) -> Result<Bytes, BlobError> {
        mark("blob_reader");
        self.heads.fetch_add(1, Ordering::SeqCst);
        self.largest_head.fetch_max(max_bytes, Ordering::SeqCst);
        self.inner
            .read_head(
                blob_id,
                wrapped_key,
                master_key_id,
                plaintext_size,
                max_bytes,
            )
            .await
    }
}

/// A pause of the next `publish`: the decorator reports `reached`, then waits for `resume`.
struct Pause {
    reached: oneshot::Sender<()>,
    resume: oneshot::Receiver<()>,
}

/// 02's real `publish`, optionally paused once before it delegates.
pub struct PausingCommit {
    inner: PgTreeService,
    pause: Mutex<Option<Pause>>,
}

impl PausingCommit {
    pub fn new(inner: PgTreeService) -> Self {
        PausingCommit {
            inner,
            pause: Mutex::new(None),
        }
    }

    /// The real 02 service: its `TreeService` mutations run on the rig's database.
    pub fn tree(&self) -> &PgTreeService {
        &self.inner
    }

    /// Pauses the next `publish`; returns the signal that it was reached and the sender that lets it continue.
    pub fn pause_next(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (reached_tx, reached_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = oneshot::channel();
        *self.pause.lock().unwrap_or_else(PoisonError::into_inner) = Some(Pause {
            reached: reached_tx,
            resume: resume_rx,
        });
        (reached_rx, resume_tx)
    }
}

#[async_trait]
impl TreeCommit for PausingCommit {
    async fn publish(
        &self,
        actor: CommitActor,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
        now: Timestamp,
    ) -> Result<PublishResult, TreeCommitError> {
        let pause = self
            .pause
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(Pause { reached, resume }) = pause {
            reached.send(()).expect("the test waits for the pause");
            resume.await.expect("the test resumes the publish");
        }
        self.inner.publish(actor, entries, effects, now).await
    }
}
