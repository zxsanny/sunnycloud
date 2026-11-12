use crate::{RandomSource, VaultError};

/// The operating-system random source. Native targets only; the only code of the crate that reads system
/// randomness (AC-3 check in `tests/randomness.rs`).
#[derive(Debug, Default)]
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), VaultError> {
        getrandom::fill(dest).map_err(|_os_error| VaultError::Rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_source_fills_the_whole_buffer() {
        // Arrange
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];

        // Act
        let filled = OsRandom.fill(&mut first).and(OsRandom.fill(&mut second));

        // Assert
        assert_eq!(filled, Ok(()));
        assert_ne!(first, second);
    }
}
