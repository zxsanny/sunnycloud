use thiserror::Error;

/// Failures of the pure format code: identifiers, sizes, counters and key material. A chunk that does not
/// verify is not here; it is `BlobError::Corrupt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FormatError {
    #[error("not a lowercase hyphenated v4 blob id")]
    InvalidBlobId,
    /// The chunk counter is 32 bits and its maximum is never used, so a blob has at most 2^32 - 1 chunks.
    #[error("chunk counter limit reached")]
    CounterLimit,
    #[error("chunk index is past the last chunk of the blob")]
    ChunkOutOfRange,
    #[error("chunk length does not match the declared size")]
    ChunkLength,
    #[error("operating system random source unavailable")]
    RandomUnavailable,
    #[error("cipher failure")]
    Cipher,
}
