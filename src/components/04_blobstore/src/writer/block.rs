//! One open blob file and the sealing of a 2 MiB block into it.

use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, ErrorKind};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use contracts::{BlobError, BlobPlan};
use uuid::Uuid;

use crate::chunk::ChunkCipher;
use crate::data_key::DataKey;
use crate::error::{BlobIds, blob_error_from_io};
use crate::format_error::FormatError;
use crate::header::{HEADER_LEN, Header};
use crate::keyring::MasterKeyring;
use crate::layout::{BlobLayout, CHUNK_SIZE, TAG_LEN};
use crate::path::blob_path;

pub(super) const BLOCK_CHUNKS: u64 = 32;
pub(super) const BLOCK_LEN: usize = (CHUNK_SIZE * BLOCK_CHUNKS) as usize;

pub(super) fn blob_ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

pub(super) fn format_failure(error: FormatError, ids: BlobIds) -> BlobError {
    match error {
        FormatError::ChunkLength => BlobError::SizeMismatch,
        FormatError::ChunkOutOfRange | FormatError::CounterLimit => BlobError::OutOfRange,
        FormatError::InvalidBlobId | FormatError::RandomUnavailable | FormatError::Cipher => {
            blob_error_from_io(io::Error::other(error.to_string()), ids)
        }
    }
}

/// A declared size the format can hold; a negative size or one past the chunk counter is `OutOfRange`.
pub(super) fn layout_of(size: i64) -> Result<BlobLayout, BlobError> {
    let size = u64::try_from(size).map_err(|_| BlobError::OutOfRange)?;
    BlobLayout::new(size).map_err(|_| BlobError::OutOfRange)
}

/// `OutOfRange` for a block past the declared size, `SizeMismatch` for a block of the wrong length.
pub(super) fn check_block(
    layout: &BlobLayout,
    block_index: u32,
    len: usize,
) -> Result<(), BlobError> {
    match block_len(layout, block_index) {
        None => Err(BlobError::OutOfRange),
        Some(expected) if expected != len as u64 => Err(BlobError::SizeMismatch),
        Some(_) => Ok(()),
    }
}

/// Plaintext bytes the block holds: `None` when the block starts past the last chunk.
pub(super) fn block_len(layout: &BlobLayout, block_index: u32) -> Option<u64> {
    let first = u64::from(block_index) * BLOCK_CHUNKS;
    if first >= layout.chunk_count() {
        return None;
    }
    let end = layout.size().min((first + BLOCK_CHUNKS) * CHUNK_SIZE);
    Some(end - first * CHUNK_SIZE)
}

pub(super) struct OpenBlob {
    pub file: Arc<File>,
    cipher: ChunkCipher,
    layout: BlobLayout,
    blob_id: Uuid,
}

impl OpenBlob {
    pub fn new(
        file: Arc<File>,
        key: &DataKey,
        header: &Header,
        blob_id: Uuid,
        layout: BlobLayout,
    ) -> Self {
        OpenBlob {
            file,
            cipher: ChunkCipher::new(key, header.nonce_prefix, blob_id),
            layout,
            blob_id,
        }
    }

    /// Opens the existing file, reads its nonce prefix and unwraps the data key under the row's master key id.
    pub fn open(
        root: &Path,
        keyring: &MasterKeyring,
        plan: &BlobPlan,
        layout: BlobLayout,
    ) -> Result<Self, BlobError> {
        let ids = blob_ids(plan.blob_id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(blob_path(root, plan.blob_id))
            .map_err(|error| blob_error_from_io(error, ids))?;
        let mut bytes = [0; HEADER_LEN];
        file.read_exact_at(&mut bytes, 0).map_err(|error| {
            if error.kind() == ErrorKind::UnexpectedEof {
                BlobError::Corrupt
            } else {
                blob_error_from_io(error, ids)
            }
        })?;
        let header = Header::decode(&bytes)?;
        let key = keyring.unwrap_data_key(plan.blob_id, &plan.wrapped_key, plan.master_key_id)?;
        Ok(OpenBlob::new(
            Arc::new(file),
            &key,
            &header,
            plan.blob_id,
            layout,
        ))
    }

    /// The port's `write_block`: a block index is sealed at most once with one plaintext. The stored region decides:
    /// all zero (never written, preallocated or a hole) is written as a first write; a region whose every chunk
    /// authenticates is compared with `plaintext`, so identical bytes are accepted and the stored ciphertext is written
    /// back unchanged, and other bytes are `BlockConflict`; any other region (a torn write) is `BlockConflict`. The
    /// caller holds the block's lock.
    pub fn write_block_once(&self, block_index: u32, plaintext: &[u8]) -> Result<(), BlobError> {
        let ids = blob_ids(self.blob_id);
        check_block(&self.layout, block_index, plaintext.len())?;
        let first = u64::from(block_index) * BLOCK_CHUNKS;
        let last = (first + BLOCK_CHUNKS).min(self.layout.chunk_count());
        let offset = self
            .layout
            .chunk_offset(first)
            .ok_or(BlobError::OutOfRange)?;
        let sealed_len = plaintext.len() + ((last - first) * TAG_LEN) as usize;
        let region = read_region(&self.file, offset, sealed_len)
            .map_err(|error| blob_error_from_io(error, ids))?;
        if region.iter().all(|byte| *byte == 0) {
            return self.write_block(block_index, plaintext);
        }
        if self.holds(first, last, &region, plaintext)? {
            // The stored bytes go back unchanged: after a failed fdatasync the kernel may have marked the pages clean
            // without persisting them, and only a write makes the next sync cover them again (decided 2026-11-12).
            self.file
                .write_all_at(&region, offset)
                .map_err(|error| blob_error_from_io(error, ids))
        } else {
            Err(BlobError::BlockConflict)
        }
    }

    /// True when every stored chunk authenticates and decrypts to `plaintext`; false on the first chunk that
    /// does not authenticate or differs. The new bytes are never sealed for the comparison.
    fn holds(
        &self,
        first: u64,
        last: u64,
        region: &[u8],
        plaintext: &[u8],
    ) -> Result<bool, BlobError> {
        let mut chunk = Vec::with_capacity(CHUNK_SIZE as usize + TAG_LEN as usize);
        let mut at = 0;
        for (index, part) in (first..last).zip(plaintext_parts(plaintext)) {
            let end = at + part.len() + TAG_LEN as usize;
            chunk.clear();
            chunk.extend_from_slice(region.get(at..end).ok_or(BlobError::OutOfRange)?);
            at = end;
            match self.cipher.open_chunk(&self.layout, index, &mut chunk) {
                Ok(()) if chunk == part => {}
                Ok(()) | Err(BlobError::Corrupt) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }

    /// Seals the chunks of block `block_index` and writes them with one `pwrite` at their fixed offset. Called
    /// without the rewrite check only by `write_complete`, which writes each block of a file it just created once.
    pub fn write_block(&self, block_index: u32, plaintext: &[u8]) -> Result<(), BlobError> {
        let ids = blob_ids(self.blob_id);
        check_block(&self.layout, block_index, plaintext.len())?;
        let first = u64::from(block_index) * BLOCK_CHUNKS;
        let last = (first + BLOCK_CHUNKS).min(self.layout.chunk_count());
        let mut sealed = Vec::with_capacity(plaintext.len() + ((last - first) * TAG_LEN) as usize);
        let mut chunk = Vec::with_capacity(CHUNK_SIZE as usize + TAG_LEN as usize);
        for (index, part) in (first..last).zip(plaintext_parts(plaintext)) {
            chunk.clear();
            chunk.extend_from_slice(part);
            self.cipher
                .seal_chunk(&self.layout, index, &mut chunk)
                .map_err(|error| format_failure(error, ids))?;
            sealed.extend_from_slice(&chunk);
        }
        let offset = self
            .layout
            .chunk_offset(first)
            .ok_or(BlobError::OutOfRange)?;
        self.file
            .write_all_at(&sealed, offset)
            .map_err(|error| blob_error_from_io(error, ids))
    }
}

/// `len` bytes at `offset`; bytes past the end of the file read as zero, as a hole does.
fn read_region(file: &File, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    let mut region = vec![0; len];
    let mut filled = 0;
    while filled < len {
        match file.read_at(&mut region[filled..], offset + filled as u64) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(region)
}

/// Serialises the read-compare-write of one block index across the tasks of this writer, so two writers of one
/// index cannot both find the region empty. Striped: indices that share a stripe wait on each other briefly,
/// which costs less than a map of per-index locks that must be cleaned up.
pub(super) struct BlockLocks([Mutex<()>; LOCK_STRIPES]);

const LOCK_STRIPES: usize = 256;

impl BlockLocks {
    pub fn new() -> Self {
        BlockLocks(std::array::from_fn(|_| Mutex::new(())))
    }

    pub fn lock(&self, blob_id: Uuid, block_index: u32) -> MutexGuard<'_, ()> {
        let mut hasher = DefaultHasher::new();
        (blob_id, block_index).hash(&mut hasher);
        let stripe = (hasher.finish() % LOCK_STRIPES as u64) as usize;
        // The lock guards no data, so a holder that panicked leaves nothing to repair.
        self.0[stripe]
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The 64 KiB parts of a block; an empty block (the empty blob) is one empty part.
fn plaintext_parts(plaintext: &[u8]) -> Vec<&[u8]> {
    if plaintext.is_empty() {
        vec![plaintext]
    } else {
        plaintext.chunks(CHUNK_SIZE as usize).collect()
    }
}
