//! Block arithmetic of a large upload: 2 MiB blocks (the protocol constant [`BLOCK_SIZE`]), the last one
//! shorter, and the `received` bitmap layout (bit `i` is bit `i % 8` of byte `i / 8`).

use contracts::UploadError;
use sc_ids::BLOCK_SIZE;

/// The 2 MiB protocol block size as the port's `block_size`.
pub(crate) const BLOCK_SIZE_U32: u32 = BLOCK_SIZE as u32;

/// `ceil(declared_size / 2 MiB)`; the configured size ceiling keeps it within the `integer` column.
pub(crate) fn block_count(declared_size: i64) -> Result<i32, UploadError> {
    let block = i64::from(BLOCK_SIZE_U32);
    let blocks = declared_size / block + i64::from(declared_size % block > 0);
    i32::try_from(blocks).map_err(|_| UploadError::SizeTooLarge)
}

/// The length block `index` must have: `min(2 MiB, remaining)`; `None` when the block starts at or past the
/// declared size.
pub(crate) fn expected_len(declared_size: i64, index: u32) -> Option<u64> {
    let size = u64::try_from(declared_size).ok()?;
    let start = u64::from(index) * u64::from(BLOCK_SIZE_U32);
    let remaining = size.checked_sub(start).filter(|left| *left > 0)?;
    Some(remaining.min(u64::from(BLOCK_SIZE_U32)))
}

/// The indexes whose bit is clear in a `received` bitmap.
pub(crate) fn need_blocks(block_count: i32, received: &[u8]) -> Vec<u32> {
    (0..as_u32(block_count))
        .filter(|index| {
            let byte = received.get(*index as usize / 8).copied().unwrap_or(0);
            byte & (1 << (index % 8)) == 0
        })
        .collect()
}

pub(crate) fn as_u32(block_count: i32) -> u32 {
    u32::try_from(block_count).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: i64 = 1 << 20;

    #[test]
    fn block_count_is_the_ceiling_of_two_mib_blocks() {
        // Arrange
        let cases = [
            (1, 1),
            (2 * MIB - 1, 1),
            (2 * MIB, 1),
            (2 * MIB + 1, 2),
            (4 * MIB, 2),
            (10 * MIB, 5),
            (10 * MIB + 1, 6),
        ];

        for (size, expected) in cases {
            // Act
            let blocks = block_count(size).expect("block count");

            // Assert
            assert_eq!(blocks, expected, "size {size}");
        }
    }

    #[test]
    fn expected_length_is_two_mib_or_what_remains() {
        // Arrange
        let two = 2 * MIB as u64;
        let cases = [
            (9 * MIB, 0, Some(two)),
            (9 * MIB, 3, Some(two)),
            (9 * MIB, 4, Some(MIB as u64)),
            (9 * MIB, 5, None),
            (8 * MIB, 3, Some(two)),
            (8 * MIB, 4, None),
            (4 * MIB + 1, 2, Some(1)),
            (1, 0, Some(1)),
            (0, 0, None),
            (9 * MIB, u32::MAX, None),
        ];

        for (size, index, expected) in cases {
            // Act
            let length = expected_len(size, index);

            // Assert
            assert_eq!(length, expected, "size {size} index {index}");
        }
    }

    #[test]
    fn a_new_session_needs_every_block() {
        // Arrange
        let blocks = block_count(10 * MIB).expect("block count");

        // Act
        let need = need_blocks(blocks, &[0]);

        // Assert
        assert_eq!(need, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn received_bits_are_not_needed_again() {
        // Arrange
        let received = [0b0000_0101, 0b0000_0001];

        // Act
        let need = need_blocks(9, &received);

        // Assert
        assert_eq!(need, vec![1, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_count_beyond_the_integer_column_is_too_large() {
        // Arrange
        let size = (i64::from(i32::MAX) + 1) * i64::from(BLOCK_SIZE_U32);

        // Act
        let result = block_count(size);

        // Assert
        assert!(matches!(result, Err(UploadError::SizeTooLarge)));
    }
}
