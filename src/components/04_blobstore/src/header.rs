use contracts::BlobError;

use crate::format_error::FormatError;

pub const HEADER_LEN: usize = 64;
/// The 24-byte XChaCha20 nonce less the 4-byte big-endian counter and the 1-byte last-chunk flag.
pub const NONCE_PREFIX_LEN: usize = 19;
pub const FORMAT_VERSION: u32 = 1;

const MAGIC: [u8; 8] = *b"SUNNYBLB";
const VERSION_AT: usize = MAGIC.len();
const PREFIX_AT: usize = VERSION_AT + 4;

pub type NoncePrefix = [u8; NONCE_PREFIX_LEN];

/// The fixed 64-byte file header: magic, format version (u32 big-endian), nonce prefix, zero pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub nonce_prefix: NoncePrefix,
}

impl Header {
    /// A header with a fresh random nonce prefix; every blob has its own prefix and its own key.
    pub fn generate() -> Result<Self, FormatError> {
        let mut nonce_prefix = [0; NONCE_PREFIX_LEN];
        getrandom::fill(&mut nonce_prefix).map_err(|_| FormatError::RandomUnavailable)?;
        Ok(Header { nonce_prefix })
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0; HEADER_LEN];
        bytes[..VERSION_AT].copy_from_slice(&MAGIC);
        bytes[VERSION_AT..PREFIX_AT].copy_from_slice(&FORMAT_VERSION.to_be_bytes());
        bytes[PREFIX_AT..PREFIX_AT + NONCE_PREFIX_LEN].copy_from_slice(&self.nonce_prefix);
        bytes
    }

    /// A header that is not exactly what [`Header::encode`] writes for version 1 is `Corrupt`; a later
    /// format version is read by a later release, not guessed at.
    pub fn decode(bytes: &[u8]) -> Result<Self, BlobError> {
        let bytes: &[u8; HEADER_LEN] = bytes.try_into().map_err(|_| BlobError::Corrupt)?;
        let mut nonce_prefix = [0; NONCE_PREFIX_LEN];
        nonce_prefix.copy_from_slice(&bytes[PREFIX_AT..PREFIX_AT + NONCE_PREFIX_LEN]);
        let header = Header { nonce_prefix };
        if header.encode() == *bytes {
            Ok(header)
        } else {
            Err(BlobError::Corrupt)
        }
    }
}
