//! How many of the next files form one batch.

/// Length of the first chunk of `sizes`: at most `max_entries` files and at most `max_bytes` bytes, but at
/// least one file, so a file above the byte limit still goes out on its own.
pub(super) fn chunk_len(
    sizes: impl Iterator<Item = u64>,
    max_entries: usize,
    max_bytes: u64,
) -> usize {
    let mut bytes = 0u64;
    let mut count = 0;
    for size in sizes.take(max_entries) {
        bytes = bytes.saturating_add(size);
        if count > 0 && bytes > max_bytes {
            break;
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::small::takes_staged_path;

    const MIB: u64 = 1024 * 1024;

    fn split(sizes: &[u64], max_entries: usize, max_bytes: u64) -> Vec<usize> {
        let mut rest = sizes;
        let mut chunks = Vec::new();
        while !rest.is_empty() {
            let len = chunk_len(rest.iter().copied(), max_entries, max_bytes);
            chunks.push(len);
            rest = &rest[len..];
        }
        chunks
    }

    #[test]
    fn size_routing_puts_the_boundary_between_2_mib_and_2_mib_plus_1() {
        // Arrange
        let sizes = [0, 2 * MIB - 1, 2 * MIB, 2 * MIB + 1];

        // Act
        let routed: Vec<bool> = sizes.iter().map(|size| takes_staged_path(*size)).collect();

        // Assert
        assert_eq!(routed, vec![true, true, true, false]);
    }

    #[test]
    fn chunks_respect_the_entry_limit() {
        // Arrange
        let sizes = vec![10; 25];

        // Act
        let chunks = split(&sizes, 10, u64::MAX);

        // Assert
        assert_eq!(chunks, vec![10, 10, 5]);
    }

    #[test]
    fn chunks_respect_the_byte_limit() {
        // Arrange
        let sizes = vec![MIB; 10];

        // Act
        let chunks = split(&sizes, 100, 4 * MIB);

        // Assert
        assert_eq!(chunks, vec![4, 4, 2]);
    }

    #[test]
    fn a_file_above_the_byte_limit_gets_a_chunk_of_its_own() {
        // Arrange
        let sizes = [2 * MIB, 3 * MIB, MIB];

        // Act
        let chunks = split(&sizes, 100, MIB);

        // Assert
        assert_eq!(chunks, vec![1, 1, 1]);
    }

    #[test]
    fn no_chunk_exceeds_either_limit_for_mixed_sizes() {
        // Arrange
        let sizes: Vec<u64> = (0..500u64).map(|n| (n * 7919) % (2 * MIB + 1)).collect();
        let (max_entries, max_bytes) = (37, 16 * MIB);

        // Act
        let chunks = split(&sizes, max_entries, max_bytes);

        // Assert
        let mut start = 0;
        for len in &chunks {
            let bytes: u64 = sizes[start..start + len].iter().sum();
            assert!(*len <= max_entries);
            assert!(*len == 1 || bytes <= max_bytes);
            start += len;
        }
        assert_eq!(start, sizes.len());
    }
}
