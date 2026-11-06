//! Host side of the small-upload tests: sources, a spying checkpoint store, a recording sink.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use sync_core::{
    BatchRecord, BatchReport, BatchSink, CheckpointStore, FileIdentity, FileSource, HttpSyncClient,
    MemoryCheckpointStore, SmallFile, StoreError, UploadCheckpoint,
};
use sync_core_proto::{BatchEntry, FileHash, hash_block, rollup};
use tokio::sync::Notify;
use uuid::Uuid;

use super::{Event, Timeline, id};
use crate::connect_support::{client, config, connect};
use crate::support::{Fixture, seeded_bytes};

/// A file held in memory; `replace` changes its content like an edit would.
pub struct MemSource(Mutex<Bytes>);

impl MemSource {
    pub fn new(bytes: Bytes) -> Arc<Self> {
        Arc::new(Self(Mutex::new(bytes)))
    }

    pub fn replace(&self, bytes: Bytes) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = bytes;
    }

    fn bytes(&self) -> Bytes {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl FileSource for MemSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.bytes().len() as u64)
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let bytes = self.bytes();
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = start.saturating_add(len as usize).min(bytes.len());
        Ok(bytes.slice(start..end))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        Ok(FileIdentity {
            size: self.bytes().len() as u64,
            mtime_ns: 0,
        })
    }
}

pub fn file_hash_of(bytes: &[u8]) -> FileHash {
    if bytes.is_empty() {
        rollup(&[])
    } else {
        rollup(&[hash_block(bytes).expect("a small body is one block")])
    }
}

/// `count` files of `size` bytes with keys `first..first + count`; a file's content depends on its number.
pub fn files(first: u128, count: usize, size: usize) -> Vec<SmallFile> {
    (0..count as u128)
        .map(|n| {
            let number = first + n;
            let bytes = seeded_bytes(number as u64, size);
            SmallFile {
                entry: BatchEntry {
                    staged_key: id(number),
                    parent: id(1),
                    name: format!("file-{number}.bin"),
                    base_revision: None,
                    file_hash: file_hash_of(&bytes),
                    client_meta: None,
                    import_hint: None,
                },
                source: MemSource::new(bytes),
            }
        })
        .collect()
}

/// Source lookup for a replay: the files by staged key.
pub fn resolver(
    files: &[SmallFile],
) -> impl Fn(&BatchEntry) -> Option<Arc<dyn FileSource>> + Sync + use<> {
    let table: Vec<(Uuid, Arc<dyn FileSource>)> = files
        .iter()
        .map(|file| (file.entry.staged_key, Arc::clone(&file.source)))
        .collect();
    move |entry| {
        table
            .iter()
            .find(|(key, _)| *key == entry.staged_key)
            .map(|(_, source)| Arc::clone(source))
    }
}

/// A new client for the same fixture: what a restarted process has.
pub async fn restart(fixture: &Fixture) -> HttpSyncClient {
    client(connect(fixture, config(&fixture.url())).await).0
}

/// Stops the thread that reached it until the test released it, so a test can drop the run exactly there.
pub struct Gate {
    pub reached: Arc<Notify>,
    release: Mutex<Receiver<()>>,
}

impl Gate {
    pub fn new() -> (Arc<Self>, std::sync::mpsc::Sender<()>) {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let gate = Arc::new(Self {
            reached: Arc::new(Notify::new()),
            release: Mutex::new(release_rx),
        });
        (gate, release_tx)
    }

    fn pass(&self) {
        self.reached.notify_one();
        let release = self.release.lock().unwrap_or_else(PoisonError::into_inner);
        release
            .recv()
            .expect("the test holds the release sender until the run is dropped");
    }
}

/// A [`MemoryCheckpointStore`] that writes its batch calls to the timeline. `gate_on_save` stops the thread
/// that saved the first batch record, after the record is stored.
pub struct SpyStore {
    inner: MemoryCheckpointStore,
    timeline: Timeline,
    gate_on_save: Option<Arc<Gate>>,
    saves: AtomicUsize,
}

impl SpyStore {
    pub fn new(timeline: &Timeline) -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryCheckpointStore::new(),
            timeline: timeline.clone(),
            gate_on_save: None,
            saves: AtomicUsize::new(0),
        })
    }

    pub fn gated_on_first_save(timeline: &Timeline, gate: Arc<Gate>) -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryCheckpointStore::new(),
            timeline: timeline.clone(),
            gate_on_save: Some(gate),
            saves: AtomicUsize::new(0),
        })
    }

    pub fn saves(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }
}

impl CheckpointStore for SpyStore {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        self.inner.load(local_key)
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.inner.save(local_key, checkpoint)
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.inner.clear(local_key)
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError> {
        let commit_id = record.commit_id;
        self.inner.save_batch(record)?;
        self.timeline.push(Event::Save(commit_id));
        if self.saves.fetch_add(1, Ordering::SeqCst) == 0
            && let Some(gate) = &self.gate_on_save
        {
            gate.pass();
        }
        Ok(())
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError> {
        self.inner.load_batches()
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.inner.clear_batch(commit_id)?;
        self.timeline.push(Event::Clear(commit_id));
        Ok(())
    }
}

/// Keeps every report, as a host that stores results would, and notes the hand-over on the timeline.
pub struct RecordingSink {
    timeline: Timeline,
    reports: Mutex<Vec<BatchReport>>,
}

impl RecordingSink {
    pub fn new(timeline: &Timeline) -> Arc<Self> {
        Arc::new(Self {
            timeline: timeline.clone(),
            reports: Mutex::new(Vec::new()),
        })
    }

    pub fn reports(&self) -> Vec<BatchReport> {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl BatchSink for RecordingSink {
    fn on_batch(&self, report: BatchReport) -> Result<(), StoreError> {
        self.timeline.push(Event::Report(report.commit_id));
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(report);
        Ok(())
    }
}
