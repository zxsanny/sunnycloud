//! The rules behind the exports as plain Rust, so the native tests run them without a JavaScript host.
//! Every function calls `sync-core-proto` (and through it `sc_ids`); nothing is re-implemented here.

use sync_core_proto::{FileHash, Hash32};
use uuid::{Builder, Uuid};

use crate::ExportError;

/// Largest UUIDv7 time: the timestamp field is 48 bits of Unix milliseconds.
pub const MAX_COMMIT_ID_MS: u64 = (1 << 48) - 1;

/// Bytes of caller randomness in a UUIDv7 (`rand_a` and `rand_b`; version and variant bits overwrite 6).
pub const COMMIT_ID_RANDOM_BYTES: usize = 10;

/// BLAKE3 of one block of at most `BLOCK_SIZE` bytes.
pub fn hash_block(block: &[u8]) -> Result<[u8; 32], ExportError> {
    sync_core_proto::hash_block(block)
        .map(|hash| *hash.as_bytes())
        .map_err(|_too_large| ExportError::BlockTooLarge)
}

/// File hash over `n x 32` bytes of ordered block hashes; empty input is the zero-length file.
pub fn rollup(block_hashes: &[u8]) -> Result<[u8; 32], ExportError> {
    if !block_hashes.len().is_multiple_of(32) {
        return Err(ExportError::InvalidLength);
    }
    let hashes = block_hashes
        .chunks_exact(32)
        .map(|chunk| hash32(chunk).map(Hash32::from_bytes))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(*sync_core_proto::rollup(&hashes).as_bytes())
}

/// A web commit ID: UUIDv7 from the caller's Unix milliseconds and 10 random bytes (the Worker passes
/// `Date.now()` and `crypto.getRandomValues`). The module reads no clock and draws no randomness.
pub fn new_commit_id(unix_ms: f64, random: &[u8]) -> Result<Uuid, ExportError> {
    let millis = whole_millis(unix_ms)?;
    let random: [u8; COMMIT_ID_RANDOM_BYTES] = random
        .try_into()
        .map_err(|_wrong| ExportError::InvalidLength)?;
    Ok(Builder::from_unix_timestamp_millis(millis, &random).into_uuid())
}

/// The mobile `client_key` of `(device_id, asset_id, file_hash)`.
pub fn client_key_for_mobile(
    device_id: &str,
    asset_id: &str,
    file_hash: &[u8],
) -> Result<Uuid, ExportError> {
    let device = Uuid::parse_str(device_id).map_err(|_not_uuid| ExportError::InvalidUuid)?;
    let file_hash = FileHash::from_bytes(hash32(file_hash)?);
    Ok(sync_core_proto::client_key_for_mobile(
        device, asset_id, &file_hash,
    ))
}

fn hash32(bytes: &[u8]) -> Result<[u8; 32], ExportError> {
    bytes
        .try_into()
        .map_err(|_wrong| ExportError::InvalidLength)
}

fn whole_millis(unix_ms: f64) -> Result<u64, ExportError> {
    let in_range = unix_ms.is_finite() && unix_ms >= 0.0 && unix_ms <= MAX_COMMIT_ID_MS as f64;
    if !in_range || unix_ms.fract() != 0.0 {
        return Err(ExportError::InvalidTime);
    }
    // Exact: a whole number below 2^48 is representable in both f64 and u64.
    Ok(unix_ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversize_block_is_refused() {
        // Arrange
        let block = vec![0u8; sync_core_proto::BLOCK_SIZE + 1];

        // Act
        let result = hash_block(&block);

        // Assert
        assert_eq!(result, Err(ExportError::BlockTooLarge));
    }

    #[test]
    fn rollup_refuses_a_partial_hash() {
        // Arrange
        let bytes = [7u8; 33];

        // Act
        let result = rollup(&bytes);

        // Assert
        assert_eq!(result, Err(ExportError::InvalidLength));
    }

    #[test]
    fn commit_id_is_v7_with_the_given_time() {
        // Arrange
        let millis = 1_731_456_000_123u64;

        // Act
        let id = new_commit_id(millis as f64, &[0xAB; 10]).expect("valid input");

        // Assert
        assert_eq!(id.get_version_num(), 7);
        let (seconds, nanos) = id
            .get_timestamp()
            .expect("v7 carries a timestamp")
            .to_unix();
        assert_eq!(seconds * 1000 + u64::from(nanos) / 1_000_000, millis);
        assert_eq!(sync_core_proto::check_batch_ids(id, &[]), Ok(()));
    }

    #[test]
    fn commit_id_refuses_bad_time_and_random_length() {
        // Arrange
        let bad_times = [
            -1.0,
            0.5,
            f64::NAN,
            f64::INFINITY,
            (MAX_COMMIT_ID_MS + 1) as f64,
        ];

        // Act
        let time_results: Vec<_> = bad_times
            .iter()
            .map(|ms| new_commit_id(*ms, &[0; 10]))
            .collect();
        let short_random = new_commit_id(0.0, &[0; 9]);

        // Assert
        assert!(
            time_results
                .iter()
                .all(|r| *r == Err(ExportError::InvalidTime))
        );
        assert_eq!(short_random, Err(ExportError::InvalidLength));
    }

    #[test]
    fn client_key_refuses_bad_device_and_hash() {
        // Arrange
        let hash = [1u8; 32];

        // Act
        let bad_device = client_key_for_mobile("not-a-uuid", "a", &hash);
        let bad_hash =
            client_key_for_mobile("0190c5f0-0000-7000-8000-000000000001", "a", &hash[..31]);

        // Assert
        assert_eq!(bad_device, Err(ExportError::InvalidUuid));
        assert_eq!(bad_hash, Err(ExportError::InvalidLength));
    }
}
