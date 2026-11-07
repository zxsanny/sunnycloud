use std::fmt;

use chacha20poly1305::aead::{self, AeadInOut, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use contracts::KeyError;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub(crate) const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// One 256-bit master key. Not `Clone`; wiped on drop; `Debug` prints a redaction.
pub(crate) struct MasterSecret([u8; MASTER_KEY_LEN]);

impl MasterSecret {
    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self, KeyError> {
        let bytes: &[u8; MASTER_KEY_LEN] = bytes.try_into().map_err(|_| KeyError::WrongLength)?;
        Ok(MasterSecret(*bytes))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(<&Key>::from(&self.0))
    }

    /// `nonce || ciphertext || tag` under a fresh random nonce. The buffer is sized up front, so the plaintext is
    /// replaced in place and no copy is left behind by a reallocation.
    pub(crate) fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, aead::Error> {
        let mut nonce = [0; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| aead::Error)?;
        let mut body = Vec::with_capacity(plaintext.len() + TAG_LEN);
        body.extend_from_slice(plaintext);
        self.cipher()
            .encrypt_in_place(&XNonce::from(nonce), aad, &mut body)?;
        let mut sealed = Vec::with_capacity(NONCE_LEN + body.len());
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&body);
        Ok(sealed)
    }

    /// The plaintext of [`MasterSecret::seal`]; any change of key, aad or bytes fails.
    pub(crate) fn open(
        &self,
        aad: &[u8],
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, aead::Error> {
        if sealed.len() < NONCE_LEN + TAG_LEN {
            return Err(aead::Error);
        }
        let (nonce, body) = sealed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| aead::Error)?;
        let mut plaintext = Zeroizing::new(body.to_vec());
        self.cipher()
            .decrypt_in_place(&XNonce::from(nonce), aad, &mut *plaintext)?;
        Ok(plaintext)
    }
}

impl Zeroize for MasterSecret {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for MasterSecret {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for MasterSecret {}

impl fmt::Debug for MasterSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterSecret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_a_redaction_and_zeroize_wipes_the_bytes() {
        let mut secret = MasterSecret::from_slice(&[0x5a; MASTER_KEY_LEN]).expect("32 bytes");

        let text = format!("{secret:?} {secret:#?}");
        secret.zeroize();

        assert!(text.contains("redacted"));
        assert!(!text.contains("90") && !text.contains("5a"));
        assert_eq!(secret.0, [0; MASTER_KEY_LEN]);
    }

    #[test]
    fn a_seal_opens_only_under_the_same_key_and_aad() {
        let secret = MasterSecret::from_slice(&[1; MASTER_KEY_LEN]).expect("32 bytes");
        let other = MasterSecret::from_slice(&[2; MASTER_KEY_LEN]).expect("32 bytes");

        let sealed = secret.seal(b"aad", b"plain").expect("seal");

        assert_eq!(
            secret.open(b"aad", &sealed).expect("open").as_slice(),
            b"plain"
        );
        assert!(secret.open(b"other", &sealed).is_err());
        assert!(other.open(b"aad", &sealed).is_err());
    }
}
