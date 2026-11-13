//! Host traits implemented in Kotlin or Swift. The Rust traits keep `io::Result`, `Bytes` and unit-struct
//! errors, which UniFFI cannot carry, so each has a foreign twin with UniFFI types and one error enum, and
//! [`Foreign`] makes a foreign implementation usable wherever the core takes the Rust trait. `ProgressSink`
//! has only UniFFI types and is exported as is.

use std::io;
use std::sync::Arc;

use bytes::Bytes;
use sc_config::Secret;
use thiserror::Error;
use uuid::Uuid;

use crate::host::{
    BatchRecord, CheckpointStore, Credential, CredentialProvider, CredentialUnavailable,
    DownloadSink, FileIdentity, FileSource, PersistFailed, StoreError, UploadCheckpoint,
};

/// Every failure a foreign host callback reports. An exception the foreign code throws that is not a
/// `HostError` arrives as `Failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error, uniffi::Error)]
pub enum HostError {
    #[error("not found")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("host call failed")]
    Failed,
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
    /// The foreign exception text may hold a file name, so only the fact is logged.
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        tracing::warn!("a host callback threw an unexpected exception");
        Self::Failed
    }
}

impl From<HostError> for io::Error {
    fn from(error: HostError) -> Self {
        let kind = match error {
            HostError::NotFound => io::ErrorKind::NotFound,
            HostError::PermissionDenied => io::ErrorKind::PermissionDenied,
            HostError::Failed => io::ErrorKind::Other,
        };
        io::Error::from(kind)
    }
}

/// `CredentialProvider` for a foreign host. Any `bearer` failure is `CredentialUnavailable`, any
/// `on_rotated` failure is `PersistFailed`.
#[uniffi::export(with_foreign)]
pub trait ForeignCredentialProvider: Send + Sync {
    fn bearer(&self) -> Result<Credential, HostError>;

    fn on_rotated(&self, access: Secret<String>, refresh: Secret<String>) -> Result<(), HostError>;
}

/// `FileSource` for a foreign host.
// The contract fixes the method set (`len`, `read_at`, `identity`); an empty file is `len() == 0`.
#[allow(clippy::len_without_is_empty)]
#[uniffi::export(with_foreign)]
pub trait ForeignFileSource: Send + Sync {
    fn len(&self) -> Result<u64, HostError>;

    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, HostError>;

    fn identity(&self) -> Result<FileIdentity, HostError>;
}

/// `DownloadSink` for a foreign host.
#[uniffi::export(with_foreign)]
pub trait ForeignDownloadSink: Send + Sync {
    fn write_verified(&self, offset: u64, bytes: Vec<u8>) -> Result<(), HostError>;

    fn finish(&self, verified: bool) -> Result<(), HostError>;
}

/// `CheckpointStore` for a foreign host; any failure is `StoreError`.
#[uniffi::export(with_foreign)]
pub trait ForeignCheckpointStore: Send + Sync {
    fn load(&self, local_key: String) -> Result<Option<UploadCheckpoint>, HostError>;
    fn save(&self, local_key: String, checkpoint: UploadCheckpoint) -> Result<(), HostError>;
    fn clear(&self, local_key: String) -> Result<(), HostError>;
    fn save_batch(&self, record: BatchRecord) -> Result<(), HostError>;
    fn load_batches(&self) -> Result<Vec<BatchRecord>, HostError>;
    fn clear_batch(&self, commit_id: Uuid) -> Result<(), HostError>;
}

/// A foreign host implementation where the core takes the Rust trait, for example
/// `Arc::new(Foreign(source)) as Arc<dyn FileSource>`.
pub struct Foreign<T: ?Sized>(pub Arc<T>);

impl CredentialProvider for Foreign<dyn ForeignCredentialProvider> {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        self.0.bearer().map_err(|_| CredentialUnavailable)
    }

    fn on_rotated(
        &self,
        access: Secret<String>,
        refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        self.0
            .on_rotated(access, refresh)
            .map_err(|_| PersistFailed)
    }
}

impl FileSource for Foreign<dyn ForeignFileSource> {
    fn len(&self) -> io::Result<u64> {
        Ok(self.0.len()?)
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        Ok(Bytes::from(self.0.read_at(offset, len)?))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        Ok(self.0.identity()?)
    }
}

impl DownloadSink for Foreign<dyn ForeignDownloadSink> {
    fn write_verified(&self, offset: u64, bytes: Bytes) -> io::Result<()> {
        Ok(self.0.write_verified(offset, Vec::from(bytes))?)
    }

    fn finish(&self, verified: bool) -> io::Result<()> {
        Ok(self.0.finish(verified)?)
    }
}

impl CheckpointStore for Foreign<dyn ForeignCheckpointStore> {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        self.0.load(local_key.to_owned()).map_err(|_| StoreError)
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.0
            .save(local_key.to_owned(), checkpoint)
            .map_err(|_| StoreError)
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.0.clear(local_key.to_owned()).map_err(|_| StoreError)
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError> {
        self.0.save_batch(record).map_err(|_| StoreError)
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError> {
        self.0.load_batches().map_err(|_| StoreError)
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.0.clear_batch(commit_id).map_err(|_| StoreError)
    }
}
