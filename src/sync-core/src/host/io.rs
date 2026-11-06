use std::io;

use bytes::Bytes;

/// Size and modification time (nanoseconds since the Unix epoch) of a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub size: u64,
    pub mtime_ns: i64,
}

/// A local file the core uploads or hashes. Re-read `identity` before commit: a change is `SourceChanged`.
// The contract fixes the method set (`len`, `read_at`, `identity`); an empty file is `len() == 0`.
#[allow(clippy::len_without_is_empty)]
pub trait FileSource: Send + Sync {
    fn len(&self) -> io::Result<u64>;

    /// At most one block (`BLOCK_SIZE`) per call; fewer bytes only at the end of the file.
    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes>;

    fn identity(&self) -> io::Result<FileIdentity>;
}

/// The target of a download. It receives verified blocks only, in order.
pub trait DownloadSink: Send + Sync {
    fn write_verified(&self, offset: u64, bytes: Bytes) -> io::Result<()>;

    /// `verified` is true only after the whole-file rollup matched.
    fn finish(&self, verified: bool) -> io::Result<()>;
}

/// Progress of one operation. `on_progress` must be cheap; the unit (bytes or blocks) is chosen by each
/// operation and counts acknowledged work only.
pub trait ProgressSink: Send + Sync {
    fn on_progress(&self, done: u64, total: u64);
}
