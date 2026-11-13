//! `VaultService` of 20 (v1.2 proposal). The header is opaque bytes: the server never parses it.

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// Entity tag of a stored header: derived from a digest of its bytes, so equal bytes give equal tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ETag(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedVault {
    pub vault_id: Uuid,
    pub etag: ETag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultHeader {
    pub header: Vec<u8>,
    pub etag: ETag,
    /// The vault's root folder, returned by the GET route next to the header.
    pub root_node_id: Uuid,
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault exists")]
    VaultExists,
    #[error("header too large")]
    HeaderTooLarge,
    /// Unknown, foreign or tombstoned.
    #[error("not found")]
    NotFound,
    /// `if_match` differs from the stored header's tag; the stored header is unchanged.
    #[error("precondition failed")]
    PreconditionFailed,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait VaultService: Send + Sync {
    async fn create_vault(
        &self,
        account: Uuid,
        header: &[u8],
        root_node_id: Uuid,
    ) -> Result<CreatedVault, VaultError>;

    async fn get_header(&self, account: Uuid, vault_id: Uuid) -> Result<VaultHeader, VaultError>;

    async fn replace_header(
        &self,
        account: Uuid,
        vault_id: Uuid,
        header: &[u8],
        if_match: &ETag,
    ) -> Result<ETag, VaultError>;

    /// Best effort: the report carries ids and time only. `NotFound` for an unknown or foreign vault.
    async fn report_unlock_failure(&self, account: Uuid, vault_id: Uuid) -> Result<(), VaultError>;
}
