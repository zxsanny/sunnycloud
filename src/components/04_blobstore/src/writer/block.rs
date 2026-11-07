//! One open blob file and the sealing of a 2 MiB block into it.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;

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

    /// Seals the chunks of block `block_index` and writes them with one `pwrite` at their fixed offset.
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

/// The 64 KiB parts of a block; an empty block (the empty blob) is one empty part.
fn plaintext_parts(plaintext: &[u8]) -> Vec<&[u8]> {
    if plaintext.is_empty() {
        vec![plaintext]
    } else {
        plaintext.chunks(CHUNK_SIZE as usize).collect()
    }
}
