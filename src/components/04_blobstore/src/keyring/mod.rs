//! The master keyring: the keys of the installation, the key check that stops a wrong key before anything is
//! served, and the wrap of per-blob data keys (XChaCha20-Poly1305, AAD = blob id, one AEAD call per request and
//! no key cache).
//!
//! `rewrap_batch` is not here, so the `MasterKeyring` port of `contracts` is not implemented yet; the methods
//! below have the signatures the port gives them.

mod keycheck;
mod keyring_repo;
mod ring;
mod secret;
mod volume;

use std::path::PathBuf;
use std::sync::OnceLock;

use contracts::{BlobError, KeyError, Keyring, MasterKey};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, AuditWriter};
use sc_db::Db;
use sc_observability::{log_error, log_info};
use uuid::Uuid;

use crate::config::{BlobstoreConfig, FILE_KEY_ID};
use crate::data_key::{DATA_KEY_LEN, DataKey};
use crate::error::{
    BlobIds, blob_error_from_io, blob_error_from_unwrap, key_error_from_audit,
    key_error_from_cipher, key_error_from_volume,
};
use keycheck::KeyCheck;
use ring::KeyRing;
use secret::MasterSecret;

const SAMPLE_SIZE: i64 = 100;

pub struct MasterKeyring {
    config: BlobstoreConfig,
    db: Db,
    audit: AuditWriter,
    ring: OnceLock<KeyRing>,
}

impl MasterKeyring {
    /// Holds no key until [`MasterKeyring::load_and_verify`] succeeds.
    pub fn new(config: BlobstoreConfig, db: Db, audit: AuditWriter) -> Self {
        MasterKeyring {
            config,
            db,
            audit,
            ring: OnceLock::new(),
        }
    }

    /// Reads the key files and checks each key against `.keycheck`. On a start without the file the check is
    /// created only when the `blobs` table and the volume are both empty. A ring id the file does not know yet is
    /// added when at least one other ring key verified (a rotation). Any error leaves the keyring empty, so no
    /// data key can be wrapped or unwrapped. A loaded keyring is returned as it is.
    pub async fn load_and_verify(&self) -> Result<Keyring, KeyError> {
        if let Some(ring) = self.ring.get() {
            return Ok(ring.view());
        }
        let ring = KeyRing::load(&self.config).await?;
        let root = self.volume_root().await?;
        match volume::read_key_check(&root).await? {
            Some(check) => update_key_check(&root, check, &ring).await?,
            None => create_key_check(&self.db, &root, &ring).await?,
        }
        log_info!("startup key check passed");
        Ok(self.ring.get_or_init(|| ring).view())
    }

    /// For a restored volume without `.keycheck`: the candidate must unwrap a random sample of up to 100
    /// committed blobs before the file is written, and an empty `blobs` table skips the sample. The candidate is
    /// key 1. A failed sample writes nothing. The success is audited as `admin_action` before the file is written.
    /// With a `.keycheck` present the candidate is only compared with it.
    pub async fn init_key_check(&self, candidate: &MasterKey) -> Result<(), KeyError> {
        let secret = MasterSecret::from_slice(&candidate.0)?;
        let root = self.volume_root().await?;
        if let Some(existing) = volume::read_key_check(&root).await? {
            return match existing.verify(FILE_KEY_ID, &secret) {
                Some(true) => Ok(()),
                _ => Err(KeyError::KeyCheckMismatch),
            };
        }
        let sample = keyring_repo::sample_committed(&self.db, SAMPLE_SIZE).await?;
        for (blob_id, wrapped) in &sample {
            if let Err(error) = secret.open(blob_id.as_bytes(), wrapped) {
                let cause = key_error_from_cipher(error, blob_ids(*blob_id));
                log_error!("key check sample failed", err = &cause);
                return Err(KeyError::KeyCheckSampleFailed { blob_id: *blob_id });
            }
        }
        let mut check = KeyCheck::default();
        check.seal(FILE_KEY_ID, &secret)?;
        let event = AuditEvent {
            detail: AuditDetail::new()
                .with_code("action", "keycheck_init")
                .with_int("sampled", i64::try_from(sample.len()).unwrap_or(i64::MAX)),
            ..AuditEvent::new(AuditEventKind::AdminAction)
        };
        self.audit
            .record(event)
            .await
            .map_err(key_error_from_audit)?;
        volume::write_key_check(&root, &check).await
    }

    /// Wraps `key` under the current key id; returns the bytes for `blobs.wrapped_key` and that id.
    pub fn wrap_data_key(&self, blob_id: Uuid, key: &DataKey) -> Result<(Vec<u8>, i16), BlobError> {
        let (id, secret) = self
            .ring
            .get()
            .and_then(KeyRing::current)
            .ok_or(BlobError::KeyUnavailable)?;
        let wrapped = secret
            .seal(blob_id.as_bytes(), key.as_bytes())
            .map_err(|_| {
                blob_error_from_io(std::io::Error::other("cipher failure"), blob_ids(blob_id))
            })?;
        Ok((wrapped, id))
    }

    /// One AEAD call. A key id the ring does not hold, a wrap made for another blob or under another key, and
    /// damaged bytes are all `KeyUnavailable`; no data key is produced.
    pub fn unwrap_data_key(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
    ) -> Result<DataKey, BlobError> {
        let Some(secret) = self.ring.get().and_then(|ring| ring.get(master_key_id)) else {
            log_error!("data key unwrap refused: master key id not held");
            return Err(BlobError::KeyUnavailable);
        };
        let plaintext = secret
            .open(blob_id.as_bytes(), wrapped_key)
            .map_err(|error| {
                let cause = key_error_from_cipher(error, blob_ids(blob_id));
                log_error!("data key unwrap failed", err = &cause);
                blob_error_from_unwrap(error)
            })?;
        let bytes: &[u8; DATA_KEY_LEN] = plaintext.as_slice().try_into().map_err(|_| {
            log_error!("unwrapped data key has the wrong length");
            BlobError::KeyUnavailable
        })?;
        Ok(DataKey::from_bytes(bytes))
    }

    async fn volume_root(&self) -> Result<PathBuf, KeyError> {
        match self.config.blob_root.as_deref() {
            Some(root) if !root.is_empty() => volume::existing_root(root).await,
            _ => Err(key_error_from_volume(std::io::Error::from(
                std::io::ErrorKind::InvalidInput,
            ))),
        }
    }
}

fn blob_ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

async fn create_key_check(db: &Db, root: &std::path::Path, ring: &KeyRing) -> Result<(), KeyError> {
    if volume::has_blob_files(root).await? || keyring_repo::any_blob(db).await? {
        return Err(KeyError::KeyCheckAbsentButBlobsExist);
    }
    let mut check = KeyCheck::default();
    for (id, secret) in ring.iter() {
        check.seal(id, secret)?;
    }
    volume::write_key_check(root, &check).await
}

async fn update_key_check(
    root: &std::path::Path,
    mut check: KeyCheck,
    ring: &KeyRing,
) -> Result<(), KeyError> {
    let mut verified = 0_usize;
    let mut unknown = Vec::new();
    for (id, secret) in ring.iter() {
        match check.verify(id, secret) {
            Some(true) => verified += 1,
            Some(false) => return Err(KeyError::KeyCheckMismatch),
            None => unknown.push((id, secret)),
        }
    }
    if verified == 0 {
        return Err(KeyError::KeyCheckMismatch);
    }
    if unknown.is_empty() {
        return Ok(());
    }
    for (id, secret) in unknown {
        check.seal(id, secret)?;
    }
    volume::write_key_check(root, &check).await
}
