use std::fs::File;
use std::io;
use std::os::unix::fs::{FileExt, MetadataExt};

use bytes::Bytes;
use sync_core::{FileIdentity, FileSource};

use super::super::{RelPath, TempFile};

/// A file opened by `open_for_read`; sync-core hashes and uploads from it.
#[derive(Debug)]
pub struct LinuxSource {
    file: File,
}

impl LinuxSource {
    pub(super) fn new(file: File) -> Self {
        Self { file }
    }
}

impl FileSource for LinuxSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let mut buf = vec![0u8; len as usize];
        let mut filled = 0;
        while filled < buf.len() {
            let n = self
                .file
                .read_at(&mut buf[filled..], offset + filled as u64)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        Ok(Bytes::from(buf))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        let meta = self.file.metadata()?;
        Ok(FileIdentity {
            size: meta.len(),
            mtime_ns: mtime_ns(&meta),
        })
    }
}

pub(super) fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.mtime()
        .saturating_mul(1_000_000_000)
        .saturating_add(meta.mtime_nsec())
}

#[derive(Debug)]
pub struct LinuxTemp {
    pub(super) file: File,
    pub(super) dir: RelPath,
    pub(super) name: String,
}

impl TempFile for LinuxTemp {
    fn name(&self) -> &str {
        &self.name
    }

    fn write_all_at(&self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all_at(bytes, offset)
    }
}
