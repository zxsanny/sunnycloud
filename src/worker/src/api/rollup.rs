use sc_ids::{BLOCK_SIZE, FileHash, Hash32, hash_block, rollup};

/// Streaming file hash: bytes go in as they arrive, each full 2 MiB block is hashed and dropped, and the
/// block hashes roll up into the file hash. Memory is one block plus 32 bytes per block.
pub(super) struct BlockRollup {
    block: Vec<u8>,
    hashes: Vec<Hash32>,
}

impl BlockRollup {
    pub(super) fn new() -> Self {
        Self {
            block: Vec::with_capacity(BLOCK_SIZE),
            hashes: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let room = BLOCK_SIZE - self.block.len();
            let (head, rest) = bytes.split_at(room.min(bytes.len()));
            self.block.extend_from_slice(head);
            if self.block.len() == BLOCK_SIZE {
                self.close_block();
            }
            bytes = rest;
        }
    }

    pub(super) fn finish(mut self) -> FileHash {
        if !self.block.is_empty() {
            self.close_block();
        }
        rollup(&self.hashes)
    }

    fn close_block(&mut self) {
        self.hashes.push(hash_block(&self.block));
        self.block.clear();
    }
}

#[cfg(test)]
mod tests {
    use sc_ids::FileHasher;

    use super::*;

    fn through_hasher(bytes: &[u8]) -> FileHash {
        let mut hasher = FileHasher::new();
        for block in bytes.chunks(BLOCK_SIZE) {
            hasher.update(block).expect("block fits");
            hasher.finish_block();
        }
        hasher.finish().1
    }

    #[test]
    fn any_split_of_the_input_rolls_up_to_the_same_hash() {
        // Arrange: lengths around the block edges, pushed in sizes that straddle them.
        for length in [
            0,
            1,
            BLOCK_SIZE - 1,
            BLOCK_SIZE,
            BLOCK_SIZE + 1,
            2 * BLOCK_SIZE,
        ] {
            let bytes: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
            for piece in [1_000_003, 65_536, BLOCK_SIZE + 5] {
                let mut rollup = BlockRollup::new();

                // Act
                for part in bytes.chunks(piece) {
                    rollup.push(part);
                }

                // Assert
                assert_eq!(
                    rollup.finish(),
                    through_hasher(&bytes),
                    "{length} / {piece}"
                );
            }
        }
    }
}
