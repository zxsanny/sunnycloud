use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::format_error::FormatError;

pub const DATA_KEY_LEN: usize = 32;

/// The random 256-bit key of one blob. Not `Clone` or `Copy`; wiped on drop; `Debug` prints a redaction.
pub struct DataKey([u8; DATA_KEY_LEN]);

impl DataKey {
    pub fn generate() -> Result<Self, FormatError> {
        let mut key = DataKey([0; DATA_KEY_LEN]);
        getrandom::fill(&mut key.0).map_err(|_| FormatError::RandomUnavailable)?;
        Ok(key)
    }

    /// For a key unwrapped by the keyring.
    pub fn from_bytes(bytes: &[u8; DATA_KEY_LEN]) -> Self {
        DataKey(*bytes)
    }

    /// For wrapping the key with the master key.
    pub fn as_bytes(&self) -> &[u8; DATA_KEY_LEN] {
        &self.0
    }
}

impl Zeroize for DataKey {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for DataKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for DataKey {}

impl fmt::Debug for DataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DataKey(<redacted>)")
    }
}
