use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use contracts::BlobError;
use uuid::Uuid;

use crate::data_key::DataKey;
use crate::error::blob_error_from_chunk;
use crate::format_error::FormatError;
use crate::header::{NONCE_PREFIX_LEN, NoncePrefix};
use crate::layout::{BlobLayout, COUNTER_LIMIT, TAG_LEN};

const NONCE_LEN: usize = 24;
const COUNTER_AT: usize = NONCE_PREFIX_LEN;
const FLAG_AT: usize = COUNTER_AT + 4;

/// XChaCha20-Poly1305 sealing of single chunks in the STREAM-BE32 nonce layout: prefix, 32-bit big-endian
/// chunk counter, one flag byte that is 1 on the last chunk. The sequential `aead-stream` encryptor cannot
/// jump to chunk `i`; this builds the same nonce for any `i`. The AAD is the 16 bytes of the blob id.
pub struct ChunkCipher {
    cipher: XChaCha20Poly1305,
    prefix: NoncePrefix,
    blob_id: Uuid,
}

impl ChunkCipher {
    pub fn new(key: &DataKey, prefix: NoncePrefix, blob_id: Uuid) -> Self {
        let cipher = XChaCha20Poly1305::new(<&Key>::from(key.as_bytes()));
        ChunkCipher {
            cipher,
            prefix,
            blob_id,
        }
    }

    fn nonce(&self, index: u64, last: bool) -> Result<XNonce, FormatError> {
        let counter = u32::try_from(index)
            .ok()
            .filter(|counter| u64::from(*counter) < COUNTER_LIMIT)
            .ok_or(FormatError::CounterLimit)?;
        let mut nonce = [0; NONCE_LEN];
        nonce[..NONCE_PREFIX_LEN].copy_from_slice(&self.prefix);
        nonce[COUNTER_AT..FLAG_AT].copy_from_slice(&counter.to_be_bytes());
        nonce[FLAG_AT] = u8::from(last);
        Ok(XNonce::from(nonce))
    }

    /// Replaces the plaintext in `buffer` with ciphertext and tag. An index at `u32::MAX` or above is
    /// refused: the counter never wraps.
    pub fn seal(&self, index: u64, last: bool, buffer: &mut Vec<u8>) -> Result<(), FormatError> {
        let nonce = self.nonce(index, last)?;
        self.cipher
            .encrypt_in_place(&nonce, self.blob_id.as_bytes(), buffer)
            .map_err(|_| FormatError::Cipher)
    }

    /// Replaces the ciphertext and tag in `buffer` with plaintext. Any chunk that does not verify under this
    /// blob id, index and flag is `Corrupt`; an index the counter cannot hold is `OutOfRange`.
    pub fn open(&self, index: u64, last: bool, buffer: &mut Vec<u8>) -> Result<(), BlobError> {
        let nonce = self.nonce(index, last).map_err(|_| BlobError::OutOfRange)?;
        self.cipher
            .decrypt_in_place(&nonce, self.blob_id.as_bytes(), buffer)
            .map_err(blob_error_from_chunk)
    }

    /// [`ChunkCipher::seal`] with the flag and the expected plaintext length taken from the layout.
    pub fn seal_chunk(
        &self,
        layout: &BlobLayout,
        index: u64,
        buffer: &mut Vec<u8>,
    ) -> Result<(), FormatError> {
        let expected = layout
            .plain_len(index)
            .ok_or(FormatError::ChunkOutOfRange)?;
        if buffer.len() as u64 != expected {
            return Err(FormatError::ChunkLength);
        }
        self.seal(index, layout.is_last(index), buffer)
    }

    /// [`ChunkCipher::open`] with the flag taken from the layout; a chunk of the wrong length is `Corrupt`.
    pub fn open_chunk(
        &self,
        layout: &BlobLayout,
        index: u64,
        buffer: &mut Vec<u8>,
    ) -> Result<(), BlobError> {
        let expected = layout.plain_len(index).ok_or(BlobError::OutOfRange)?;
        if buffer.len() as u64 != expected + TAG_LEN {
            return Err(BlobError::Corrupt);
        }
        self.open(index, layout.is_last(index), buffer)
    }
}
