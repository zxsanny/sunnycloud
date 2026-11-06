use contracts::{BlobError, ByteRange};

use crate::format_error::FormatError;
use crate::header::HEADER_LEN;

pub const CHUNK_SIZE: u64 = 64 * 1024;
pub const TAG_LEN: u64 = 16;
pub const SEALED_CHUNK_LEN: u64 = CHUNK_SIZE + TAG_LEN;
/// Chunk indices stay below this; `u32::MAX` itself is never used as a counter.
pub const COUNTER_LIMIT: u64 = u32::MAX as u64;

/// The chunks a byte range touches and where the range starts inside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSpan {
    pub first_chunk: u64,
    pub last_chunk: u64,
    /// Bytes of the first chunk's plaintext that precede the range.
    pub skip: usize,
    /// Bytes in the range.
    pub len: u64,
}

/// The file arithmetic of one blob, fixed by its declared plaintext size. An empty blob still has one
/// chunk, holding no bytes and only a tag, so the last-chunk flag exists for every blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobLayout {
    size: u64,
}

impl BlobLayout {
    /// A size whose last chunk index would reach the counter limit is refused.
    pub fn new(size: u64) -> Result<Self, FormatError> {
        if chunks_for(size) > COUNTER_LIMIT {
            return Err(FormatError::CounterLimit);
        }
        Ok(BlobLayout { size })
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn chunk_count(&self) -> u64 {
        chunks_for(self.size)
    }

    pub fn file_len(&self) -> u64 {
        HEADER_LEN as u64 + self.chunk_count() * TAG_LEN + self.size
    }

    pub fn is_last(&self, index: u64) -> bool {
        index + 1 == self.chunk_count()
    }

    /// Plaintext bytes of chunk `index`; `None` past the last chunk.
    pub fn plain_len(&self, index: u64) -> Option<u64> {
        if index >= self.chunk_count() {
            None
        } else if self.is_last(index) {
            Some(self.size - index * CHUNK_SIZE)
        } else {
            Some(CHUNK_SIZE)
        }
    }

    /// Where chunk `index` starts in the file; `None` past the last chunk.
    pub fn chunk_offset(&self, index: u64) -> Option<u64> {
        (index < self.chunk_count()).then(|| HEADER_LEN as u64 + index * SEALED_CHUNK_LEN)
    }

    /// Maps the inclusive range to chunks. A range that is empty, inverted, or reaches the declared size is
    /// not satisfiable.
    pub fn map_range(&self, range: ByteRange) -> Result<RangeSpan, BlobError> {
        let ByteRange { start, end } = range;
        if start > end || end >= self.size {
            return Err(BlobError::RangeNotSatisfiable);
        }
        Ok(RangeSpan {
            first_chunk: start / CHUNK_SIZE,
            last_chunk: end / CHUNK_SIZE,
            skip: (start % CHUNK_SIZE) as usize,
            len: end - start + 1,
        })
    }
}

fn chunks_for(size: u64) -> u64 {
    size.div_ceil(CHUNK_SIZE).max(1)
}
