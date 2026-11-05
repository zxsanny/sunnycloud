//! Hashing wrappers over `sc_ids`; there is no second implementation here.

use sc_ids::{FileHash, FileHasher, Hash32, HashError, rollup};

/// BLAKE3 of one block; more than `BLOCK_SIZE` bytes is `BlockTooLarge`.
pub fn hash_block(block: &[u8]) -> Result<Hash32, HashError> {
    if block.len() > sc_ids::BLOCK_SIZE {
        return Err(HashError::BlockTooLarge);
    }
    Ok(sc_ids::hash_block(block))
}

/// Hashing progress at a block boundary: the hashes of the first `blocks_done` blocks.
///
/// The caller persists it and hands it back to [`ResumableFileHasher::resume`]; reading restarts at
/// [`ResumeHashState::resume_offset`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResumeHashState {
    pub blocks_done: u64,
    pub block_hashes: Vec<Hash32>,
}

impl ResumeHashState {
    pub fn from_hashes(block_hashes: Vec<Hash32>) -> Self {
        Self {
            blocks_done: block_hashes.len() as u64,
            block_hashes,
        }
    }

    /// Byte offset of the first block that is not hashed yet.
    pub fn resume_offset(&self) -> u64 {
        self.blocks_done * sc_ids::BLOCK_SIZE as u64
    }
}

/// [`FileHasher`] that can start after already-hashed blocks.
#[derive(Debug, Default)]
pub struct ResumableFileHasher {
    done: Vec<Hash32>,
    rest: FileHasher,
}

impl ResumableFileHasher {
    pub fn resume(state: ResumeHashState) -> Self {
        Self {
            done: state.block_hashes,
            rest: FileHasher::new(),
        }
    }

    /// Adds bytes to the current block; see [`FileHasher::update`].
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), HashError> {
        self.rest.update(bytes)
    }

    /// Closes the current block; see [`FileHasher::finish_block`].
    pub fn finish_block(&mut self) -> Hash32 {
        self.rest.finish_block()
    }

    /// All block hashes, the resumed ones first, and the file hash.
    pub fn finish(self) -> (Vec<Hash32>, FileHash) {
        let (rest, _) = self.rest.finish();
        let mut all = self.done;
        all.extend(rest);
        let file_hash = rollup(&all);
        (all, file_hash)
    }
}
