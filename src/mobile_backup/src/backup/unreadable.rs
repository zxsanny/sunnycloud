//! The source of an asset the host failed to open while a batch was replayed: any read fails with `Io`, so sync-core
//! hands the file back and the loop ends the asset `failed`, as it does when the host fails at the first open.

use std::io::{self, ErrorKind};

use bytes::Bytes;
use sync_core::{FileIdentity, FileSource};

pub(super) struct Unreadable;

fn failure() -> io::Error {
    io::Error::from(ErrorKind::Other)
}

impl FileSource for Unreadable {
    fn len(&self) -> io::Result<u64> {
        Err(failure())
    }

    fn read_at(&self, _offset: u64, _len: u32) -> io::Result<Bytes> {
        Err(failure())
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        Err(failure())
    }
}
