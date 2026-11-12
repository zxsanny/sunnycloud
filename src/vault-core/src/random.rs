use crate::VaultError;

/// The only source of keys, salts and nonces. The crate never reads system randomness itself: native callers
/// may pass the OS-backed source (feature `os-rng`), WASM callers pass bytes from `crypto.getRandomValues`,
/// and tests pass a deterministic source.
pub trait RandomSource {
    /// Fills all of `dest`, or returns `VaultError::Rng`; on failure the caller must emit no output.
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), VaultError>;
}
