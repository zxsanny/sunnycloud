use std::collections::BTreeMap;
use std::io::{self, ErrorKind};

use contracts::{KeyError, Keyring};
use zeroize::Zeroizing;

use super::secret::MasterSecret;
use crate::config::{BlobstoreConfig, FILE_KEY_ID, parse_ring};
use crate::error::{BlobIds, key_error_from_io, key_error_from_volume};

/// The master keys of the installation by key id, and the id new data keys are wrapped under.
pub(crate) struct KeyRing {
    keys: BTreeMap<i16, MasterSecret>,
    current: i16,
}

impl KeyRing {
    /// `master_key_file` is key 1, `master_key_ring` adds the others; each file holds exactly 32 raw bytes.
    /// `master_key_current` defaults to the highest id. A ring that [`BlobstoreConfig::validate`] refuses is
    /// `Unexpected`.
    pub(crate) async fn load(config: &BlobstoreConfig) -> Result<Self, KeyError> {
        let mut paths: BTreeMap<i16, String> = BTreeMap::new();
        paths.insert(FILE_KEY_ID, config.master_key_file.expose().clone());
        if let Some(ring) = &config.master_key_ring {
            for (id, path) in parse_ring(ring).ok_or_else(invalid_config)? {
                match paths.insert(id, path.to_owned()) {
                    Some(other) if other != path => return Err(invalid_config()),
                    _ => {}
                }
            }
        }
        let mut keys = BTreeMap::new();
        for (id, path) in &paths {
            keys.insert(*id, read_key(path).await?);
        }
        let current = match config.master_key_current {
            Some(id) => i16::try_from(id).map_err(|_| invalid_config())?,
            None => *paths.keys().next_back().ok_or_else(invalid_config)?,
        };
        if !keys.contains_key(&current) {
            return Err(invalid_config());
        }
        Ok(KeyRing { keys, current })
    }

    pub(crate) fn view(&self) -> Keyring {
        Keyring {
            current_key_id: self.current,
            key_ids: self.keys.keys().copied().collect(),
        }
    }

    pub(crate) fn get(&self, id: i16) -> Option<&MasterSecret> {
        self.keys.get(&id)
    }

    pub(crate) fn current(&self) -> Option<(i16, &MasterSecret)> {
        self.keys.get(&self.current).map(|key| (self.current, key))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (i16, &MasterSecret)> {
        self.keys.iter().map(|(id, key)| (*id, key))
    }
}

fn invalid_config() -> KeyError {
    key_error_from_volume(io::Error::from(ErrorKind::InvalidInput))
}

async fn read_key(path: &str) -> Result<MasterSecret, KeyError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| key_error_from_io(error, BlobIds::default()))?;
    MasterSecret::from_slice(&Zeroizing::new(bytes))
}
