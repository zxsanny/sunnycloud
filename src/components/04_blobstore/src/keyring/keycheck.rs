//! The `.keycheck` file: per key id, the sealed constant that proves a master key belongs to this installation.
//!
//! Layout: magic `SCKC`, version byte, then entries of `key id (i16, big endian) || nonce || sealed constant`,
//! all of one length. The key id is the AAD, so an entry cannot be moved to another id.

use std::collections::BTreeMap;

use contracts::KeyError;

use super::secret::MasterSecret;

const MAGIC: &[u8; 4] = b"SCKC";
const VERSION: u8 = 1;
const CONSTANT: &[u8] = b"sunnycloud key check v1";
const ID_LEN: usize = 2;
const SEALED_LEN: usize = 24 + CONSTANT.len() + 16;
const ENTRY_LEN: usize = ID_LEN + SEALED_LEN;
const HEADER_LEN: usize = MAGIC.len() + 1;

#[derive(Default)]
pub(crate) struct KeyCheck {
    entries: BTreeMap<i16, Vec<u8>>,
}

impl KeyCheck {
    /// `None` when the bytes are not a key check file of this version. Whether a key matches is [`KeyCheck::verify`].
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let body = bytes
            .strip_prefix(MAGIC.as_slice())?
            .strip_prefix(&[VERSION])?;
        if bytes.len() < HEADER_LEN || !body.len().is_multiple_of(ENTRY_LEN) {
            return None;
        }
        let mut check = KeyCheck::default();
        for entry in body.chunks_exact(ENTRY_LEN) {
            let (id, sealed) = entry.split_at(ID_LEN);
            let id = i16::from_be_bytes(id.try_into().ok()?);
            if check.entries.insert(id, sealed.to_vec()).is_some() {
                return None;
            }
        }
        Some(check)
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.entries.len() * ENTRY_LEN);
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        for (id, sealed) in &self.entries {
            bytes.extend_from_slice(&id.to_be_bytes());
            bytes.extend_from_slice(sealed);
        }
        bytes
    }

    /// `None` when the file holds no entry for `id`; `Some(false)` when the entry does not open under `secret`.
    pub(crate) fn verify(&self, id: i16, secret: &MasterSecret) -> Option<bool> {
        let sealed = self.entries.get(&id)?;
        let opened = secret.open(&id.to_be_bytes(), sealed);
        Some(opened.is_ok_and(|constant| constant.as_slice() == CONSTANT))
    }

    pub(crate) fn seal(&mut self, id: i16, secret: &MasterSecret) -> Result<(), KeyError> {
        let sealed = secret
            .seal(&id.to_be_bytes(), CONSTANT)
            .map_err(|error| crate::error::key_error_from_cipher(error, Default::default()))?;
        self.entries.insert(id, sealed);
        Ok(())
    }
}
