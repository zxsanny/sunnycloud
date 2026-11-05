use thiserror::Error;

/// The platform has no cryptographically secure random source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("no secure random source is available")]
pub struct RngUnavailable;

/// Failure of block hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HashError {
    /// More than [`crate::BLOCK_SIZE`] bytes were fed into one block.
    #[error("a block holds at most 2 MiB")]
    BlockTooLarge,
}
