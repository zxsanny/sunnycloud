#![allow(dead_code)]
//! Host side of the hashing tests: generated and on-disk file sources, a checkpoint spy and a progress recorder.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use sync_core::{
    CheckpointStore, FileIdentity, FileSource, HashCheckpoint, MemoryCheckpointStore, ProgressSink,
    StoreError, UploadCheckpoint,
};
use sync_core_proto::BLOCK_SIZE;
use uuid::Uuid;

pub const BLOCK: u64 = BLOCK_SIZE as u64;

/// Byte `i` of a file is `i mod 251`, the content of the shared vector file.
pub fn pattern(len: u64) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// A block handed to the core; counts the blocks alive so a test sees how many the core holds at once.
struct Tracked {
    data: Vec<u8>,
    live: Arc<Live>,
}

impl AsRef<[u8]> for Tracked {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.live.now.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub struct Live {
    now: AtomicUsize,
    pub most: AtomicUsize,
}

/// What a generated source holds.
#[derive(Clone, Copy)]
pub enum Content {
    /// Byte `i` is `i mod 251`.
    Pattern,
    /// Every byte of block `n` is `n mod 251`: cheap to generate for sources of many blocks.
    BlockFill,
}

/// A file of `len` bytes made on demand. It records the offset of every read and can change its identity
/// after a number of reads, or fail a read.
pub struct GeneratedSource {
    len: u64,
    content: Content,
    reads: Mutex<Vec<u64>>,
    identity_after: Mutex<Option<(usize, FileIdentity)>>,
    fail_at_read: Option<usize>,
    pub live: Arc<Live>,
}

impl GeneratedSource {
    pub fn new(len: u64) -> Self {
        Self::with_content(len, Content::Pattern)
    }

    pub fn with_content(len: u64, content: Content) -> Self {
        Self {
            len,
            content,
            reads: Mutex::new(Vec::new()),
            identity_after: Mutex::new(None),
            fail_at_read: None,
            live: Arc::new(Live::default()),
        }
    }

    /// After `reads` block reads, `identity()` answers `identity` instead of the original.
    pub fn changing_after(self, reads: usize, identity: FileIdentity) -> Self {
        *self
            .identity_after
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((reads, identity));
        self
    }

    /// The read with this zero-based number fails with an I/O error.
    pub fn failing_at_read(mut self, read: usize) -> Self {
        self.fail_at_read = Some(read);
        self
    }

    pub fn original_identity(&self) -> FileIdentity {
        FileIdentity {
            size: self.len,
            mtime_ns: 1_700_000_000_000_000_000,
        }
    }

    /// Offsets of the reads so far, in order.
    pub fn read_offsets(&self) -> Vec<u64> {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn most_blocks_alive(&self) -> usize {
        self.live.most.load(Ordering::SeqCst)
    }
}

impl FileSource for GeneratedSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.len)
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let number = {
            let mut reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
            reads.push(offset);
            reads.len() - 1
        };
        if self.fail_at_read == Some(number) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let end = (offset + u64::from(len)).min(self.len);
        let data: Vec<u8> = match self.content {
            Content::Pattern => (offset..end).map(|i| (i % 251) as u8).collect(),
            Content::BlockFill => {
                vec![((offset / BLOCK) % 251) as u8; usize::try_from(end - offset).unwrap_or(0)]
            }
        };
        let now = self.live.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.live.most.fetch_max(now, Ordering::SeqCst);
        Ok(Bytes::from_owner(Tracked {
            data,
            live: Arc::clone(&self.live),
        }))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        let reads = self
            .reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        match *self
            .identity_after
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Some((after, identity)) if reads >= after => Ok(identity),
            _ => Ok(self.original_identity()),
        }
    }
}

/// A file on disk, read through the operating system.
pub struct DiskSource {
    path: PathBuf,
    file: Mutex<File>,
    reads: AtomicUsize,
    grow: Mutex<Option<(usize, Vec<u8>)>>,
}

impl DiskSource {
    /// Writes `bytes` to a new file in the temporary directory; the file is removed when the source is dropped.
    pub fn create(name: &str, bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sync-core-hash-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::write(&path, bytes).expect("temporary file is writable");
        let file = File::open(&path).expect("temporary file opens");
        Self {
            path,
            file: Mutex::new(file),
            reads: AtomicUsize::new(0),
            grow: Mutex::new(None),
        }
    }

    /// Appends `bytes` to the file right after the `reads`-th block read, like a writer that is still at work.
    pub fn growing_after(self, reads: usize, bytes: &[u8]) -> Self {
        *self.grow.lock().unwrap_or_else(PoisonError::into_inner) = Some((reads, bytes.to_vec()));
        self
    }

    fn append(&self, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)?
            .write_all(bytes)
    }
}

impl Drop for DiskSource {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            eprintln!("temporary file was not removed: {error}");
        }
    }
}

impl FileSource for DiskSource {
    fn len(&self) -> io::Result<u64> {
        Ok(std::fs::metadata(&self.path)?.len())
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = Vec::new();
        file.by_ref()
            .take(u64::from(len))
            .read_to_end(&mut buffer)?;
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some((after, bytes)) = &*self.grow.lock().unwrap_or_else(PoisonError::into_inner)
            && *after == read
        {
            self.append(bytes)?;
        }
        Ok(Bytes::from(buffer))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        let meta = std::fs::metadata(&self.path)?;
        let modified = meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(FileIdentity {
            size: meta.len(),
            mtime_ns: i64::try_from(modified.as_nanos()).unwrap_or(i64::MAX),
        })
    }
}

/// A store that keeps what `save` receives, in order, and can be told to fail every save.
pub struct SpyStore {
    inner: MemoryCheckpointStore,
    saved: Mutex<Vec<(String, UploadCheckpoint)>>,
    failing: bool,
}

impl SpyStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryCheckpointStore::new(),
            saved: Mutex::new(Vec::new()),
            failing: false,
        })
    }

    pub fn failing() -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryCheckpointStore::new(),
            saved: Mutex::new(Vec::new()),
            failing: true,
        })
    }

    pub fn saved(&self) -> Vec<(String, UploadCheckpoint)> {
        self.saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl CheckpointStore for SpyStore {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        self.inner.load(local_key)
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((local_key.to_owned(), checkpoint.clone()));
        if self.failing {
            return Err(StoreError);
        }
        self.inner.save(local_key, checkpoint)
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.inner.clear(local_key)
    }

    fn save_batch(&self, record: sync_core::BatchRecord) -> Result<(), StoreError> {
        self.inner.save_batch(record)
    }

    fn load_batches(&self) -> Result<Vec<sync_core::BatchRecord>, StoreError> {
        self.inner.load_batches()
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.inner.clear_batch(commit_id)
    }
}

pub const KEY: &str = "local-key-1";

pub fn checkpoint_into(store: &Arc<SpyStore>) -> HashCheckpoint {
    HashCheckpoint {
        store: store.clone(),
        local_key: KEY.to_owned(),
        commit_id: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0042),
    }
}

/// Records every `(done, total)` it is told.
#[derive(Default)]
pub struct Progress(Mutex<Vec<(u64, u64)>>);

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn calls(&self) -> Vec<(u64, u64)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ProgressSink for Progress {
    fn on_progress(&self, done: u64, total: u64) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((done, total));
    }
}
