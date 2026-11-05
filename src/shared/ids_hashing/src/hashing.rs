use uuid::{Builder, Uuid};

use crate::error::HashError;
use crate::types::{FileHash, Hash32};

/// Fixed block size: 2 MiB (RESTRICT-11). The last block of a file may be shorter.
pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

/// Frozen for v1: changing it changes every file hash.
const FILE_CONTEXT: &str = "sunnycloud file v1";
/// Frozen for v1: changing it changes every mobile commit id.
const COMMIT_CONTEXT: &str = "sunnycloud commit v1";

/// BLAKE3 of one block.
///
/// A block holds at most [`BLOCK_SIZE`] bytes; use [`FileHasher`] when the size is not known to fit.
pub fn hash_block(block: &[u8]) -> Hash32 {
    Hash32::from_bytes(*blake3::hash(block).as_bytes())
}

/// The file hash: BLAKE3 `derive_key` with the context `sunnycloud file v1` over the ordered 32-byte block
/// hashes. An empty list is the zero-length file.
pub fn rollup(blocks: &[Hash32]) -> FileHash {
    let mut hasher = blake3::Hasher::new_derive_key(FILE_CONTEXT);
    for block in blocks {
        hasher.update(block.as_bytes());
    }
    FileHash::from_bytes(*hasher.finalize().as_bytes())
}

/// Deterministic commit id of a mobile upload: the first 16 bytes of keyed BLAKE3 over
/// `device_id || asset_id || file_hash`, as a UUID of version 8 (custom) with the RFC 9562 variant.
///
/// The BLAKE3 key is `derive_key("sunnycloud commit v1", [])`. Desktop and web use [`crate::new_id`].
pub fn derive_client_key(device_id: Uuid, asset_id: &str, file_hash: &FileHash) -> Uuid {
    let key = blake3::derive_key(COMMIT_CONTEXT, &[]);
    let mut hasher = blake3::Hasher::new_keyed(&key);
    hasher.update(device_id.as_bytes());
    hasher.update(asset_id.as_bytes());
    hasher.update(file_hash.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    Builder::from_custom_bytes(bytes).into_uuid()
}

/// Incremental hasher for one file: bytes go in per block, block hashes and the file hash come out.
///
/// Callers feed at most [`BLOCK_SIZE`] bytes between two [`FileHasher::finish_block`] calls, so memory stays
/// bounded (one hasher state plus 32 bytes per block).
#[derive(Debug)]
pub struct FileHasher {
    block: blake3::Hasher,
    block_len: usize,
    hashes: Vec<Hash32>,
}

impl Default for FileHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl FileHasher {
    pub fn new() -> Self {
        Self {
            block: blake3::Hasher::new(),
            block_len: 0,
            hashes: Vec::new(),
        }
    }

    /// Adds bytes to the current block. Nothing is consumed when the block would exceed [`BLOCK_SIZE`].
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), HashError> {
        let new_len = self
            .block_len
            .checked_add(bytes.len())
            .filter(|len| *len <= BLOCK_SIZE)
            .ok_or(HashError::BlockTooLarge)?;
        self.block.update(bytes);
        self.block_len = new_len;
        Ok(())
    }

    /// Closes the current block, records its hash and starts the next block.
    ///
    /// A block with no bytes is still a block: it is recorded as the hash of the empty input.
    pub fn finish_block(&mut self) -> Hash32 {
        let hash = Hash32::from_bytes(*self.block.finalize().as_bytes());
        self.block.reset();
        self.block_len = 0;
        self.hashes.push(hash);
        hash
    }

    /// Closes a pending partial block (if any bytes are waiting) and returns the ordered block hashes and
    /// the file hash. A file with no bytes has an empty list.
    pub fn finish(mut self) -> (Vec<Hash32>, FileHash) {
        if self.block_len > 0 {
            self.finish_block();
        }
        let file_hash = rollup(&self.hashes);
        (self.hashes, file_hash)
    }
}
