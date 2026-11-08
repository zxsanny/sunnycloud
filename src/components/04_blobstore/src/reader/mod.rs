//! The `BlobReader` port: the one decrypt path of the crate.
//!
//! The reader keeps no open files and no keys. Each `open` reads the header, unwraps the data key once, and reads
//! and authenticates the first chunk of the range before it returns; later chunks are read by a task that stays at
//! most `blob_read_ahead_chunks` chunks ahead of the consumer. No plaintext leaves this module before the tag of
//! its chunk has verified.

pub(crate) mod file;
mod finding;
pub(crate) mod finding_repo;
mod probe;
mod stream;

use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use contracts::{BlobError, BlobReader, ByteRange, PlaintextStream, async_trait};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_db::Db;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::activity::{ActivityGauge, ActivityGuard};
use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io};
use crate::keyring::MasterKeyring;
use crate::layout::{BlobLayout, CHUNK_SIZE, RangeSpan};
use file::OpenedBlob;
use finding::Findings;
#[cfg(feature = "sync-hook")]
pub use probe::ReadProbe;
#[cfg(not(feature = "sync-hook"))]
pub(crate) use probe::ReadProbe;
use stream::{ChunkStream, ReadAhead, join_failure};

const MAX_HEAD: u32 = CHUNK_SIZE as u32;

pub struct FileBlobReader {
    root: PathBuf,
    window: usize,
    keyring: Arc<MasterKeyring>,
    findings: Arc<Findings>,
    probe: Arc<ReadProbe>,
    /// Counts the open downloads for the scrub's pause; absent in a process that has no scrub gauge.
    activity: Option<ActivityGauge>,
}

impl FileBlobReader {
    /// `config.blob_root` is required. The window is `blob_read_ahead_chunks`, at least one chunk.
    pub fn new(
        config: &BlobstoreConfig,
        keyring: Arc<MasterKeyring>,
        db: Db,
        audit: AuditWriter,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), BlobIds::default())
            })?;
        let window = usize::try_from(config.blob_read_ahead_chunks).unwrap_or(usize::MAX);
        Ok(FileBlobReader {
            root: PathBuf::from(root),
            window: window.clamp(1, Semaphore::MAX_PERMITS),
            keyring,
            findings: Arc::new(Findings::new(db, audit, clock)),
            probe: Arc::default(),
            activity: None,
        })
    }

    /// Every stream `open` returns counts in `gauge` from the call until the stream is dropped.
    pub fn with_activity(mut self, gauge: ActivityGauge) -> Self {
        self.activity = Some(gauge);
        self
    }

    fn active(&self) -> Option<ActivityGuard> {
        self.activity.as_ref().map(ActivityGauge::enter)
    }

    /// The counters of this reader; the test hook of IT-04-3 and of the read-ahead bound.
    #[cfg(feature = "sync-hook")]
    pub fn probe(&self) -> Arc<ReadProbe> {
        Arc::clone(&self.probe)
    }

    /// Opens the file, unwraps the key and verifies the first chunk of the range in one blocking job. A corrupt
    /// or missing blob is recorded before the error returns.
    async fn begin(
        &self,
        request: &Request<'_>,
        first_chunk: u64,
    ) -> Result<(Arc<OpenedBlob>, Vec<u8>), BlobError> {
        let root = self.root.clone();
        let keyring = Arc::clone(&self.keyring);
        let probe = Arc::clone(&self.probe);
        let wrapped_key = request.wrapped_key.to_vec();
        let (blob_id, master_key_id, layout) =
            (request.blob_id, request.master_key_id, request.layout);
        let job = tokio::task::spawn_blocking(move || {
            let key_material = (wrapped_key.as_slice(), master_key_id);
            let blob = OpenedBlob::open(&root, &keyring, probe, key_material, blob_id, layout)?;
            let first = blob.read_chunk(first_chunk)?;
            Ok((Arc::new(blob), first))
        })
        .await
        .map_err(|_| join_failure(blob_id))
        .and_then(|outcome| outcome);
        if let Err(error) = &job {
            self.findings.note(blob_id, error).await;
        }
        job
    }
}

struct Request<'a> {
    blob_id: Uuid,
    wrapped_key: &'a [u8],
    master_key_id: i16,
    layout: BlobLayout,
}

/// A declared size the format can hold; a negative size or one past the chunk counter is `OutOfRange`.
fn layout_of(size: i64) -> Result<BlobLayout, BlobError> {
    let size = u64::try_from(size).map_err(|_| BlobError::OutOfRange)?;
    BlobLayout::new(size).map_err(|_| BlobError::OutOfRange)
}

/// The whole blob when there is no range. An empty blob has one chunk and no bytes.
fn span_of(layout: &BlobLayout, range: Option<ByteRange>) -> Result<RangeSpan, BlobError> {
    match range {
        Some(range) => layout.map_range(range),
        None => Ok(RangeSpan {
            first_chunk: 0,
            last_chunk: layout.chunk_count() - 1,
            skip: 0,
            len: layout.size(),
        }),
    }
}

#[async_trait]
impl BlobReader for FileBlobReader {
    async fn open(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        range: Option<ByteRange>,
    ) -> Result<PlaintextStream, BlobError> {
        let active = self.active();
        let layout = layout_of(plaintext_size)?;
        let span = span_of(&layout, range)?;
        let request = Request {
            blob_id,
            wrapped_key,
            master_key_id,
            layout,
        };
        let (blob, first) = self.begin(&request, span.first_chunk).await?;
        let mut head = Bytes::from(first);
        head = head.slice(span.skip..);
        head.truncate(usize::try_from(span.len).unwrap_or(usize::MAX));
        let remaining = span.len - head.len() as u64;
        let rx = (span.last_chunk > span.first_chunk).then(|| {
            ReadAhead {
                blob,
                findings: Arc::clone(&self.findings),
                probe: Arc::clone(&self.probe),
                blob_id,
                first: span.first_chunk + 1,
                last: span.last_chunk,
                remaining,
            }
            .start(self.window)
        });
        Ok(Box::pin(ChunkStream::new(
            head,
            rx,
            remaining,
            blob_id,
            Arc::clone(&self.probe),
            active,
        )))
    }

    async fn read_head(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        max_bytes: u32,
    ) -> Result<Bytes, BlobError> {
        if max_bytes > MAX_HEAD {
            return Err(BlobError::OutOfRange);
        }
        let layout = layout_of(plaintext_size)?;
        let request = Request {
            blob_id,
            wrapped_key,
            master_key_id,
            layout,
        };
        let (_, first) = self.begin(&request, 0).await?;
        let mut head = Bytes::from(first);
        head.truncate(max_bytes as usize);
        Ok(head)
    }
}
