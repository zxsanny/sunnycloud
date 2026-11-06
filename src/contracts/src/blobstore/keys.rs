//! `MasterKeyring`, `EpochMarker` and `BlobInventory`.

use std::fmt;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use super::error::KeyError;
use crate::common::Fault;

/// A master key candidate as read from a file; its length is checked by the provider. `Debug` prints a
/// placeholder.
pub struct MasterKey(pub Vec<u8>);

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}

/// The verified ring as the caller may see it: key ids only, never key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyring {
    pub current_key_id: i16,
    pub key_ids: Vec<i16>,
}

#[async_trait]
pub trait MasterKeyring: Send + Sync {
    /// Any error stops the server before it binds a listener.
    async fn load_and_verify(&self) -> Result<Keyring, KeyError>;

    /// Unwraps a sample of committed blobs with the candidate before writing `.keycheck`; a failure writes
    /// nothing.
    async fn init_key_check(&self, candidate: &MasterKey) -> Result<(), KeyError>;

    /// Re-wraps up to `n` rows in short transactions and returns how many; never rewrites files.
    async fn rewrap_batch(&self, from_id: i16, to_id: i16, n: u32) -> Result<u64, KeyError>;
}

#[derive(Debug, Error)]
pub enum EpochMarkerError {
    #[error("epoch marker i/o failure")]
    Io(#[source] Fault),
}

/// The only access to the `.epoch` marker on the blob volume.
#[async_trait]
pub trait EpochMarker: Send + Sync {
    /// Atomic: after a crash `read` returns the old or the new value.
    async fn write(&self, epoch: i64) -> Result<(), EpochMarkerError>;

    /// An absent marker is `None`.
    async fn read(&self) -> Result<Option<i64>, EpochMarkerError>;
}

#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait BlobInventory: Send + Sync {
    /// Ids of committed blobs whose file is absent, checked at a throttled rate; repeated calls cover all
    /// committed rows.
    async fn find_missing(&self, batch: u32) -> Result<Vec<Uuid>, InventoryError>;
}
