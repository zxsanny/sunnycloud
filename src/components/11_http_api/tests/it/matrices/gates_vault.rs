//! Per-account fake of 20's vault port (see [`super::gates`]): an id the caller does not own is 20's `NotFound`,
//! for a vault id and for the root folder of a create alike.

use contracts::*;
use uuid::Uuid;

use super::gates::Gate;
use crate::harness::recording::RecordingVault;

#[contracts::async_trait]
impl VaultService for Gate<RecordingVault> {
    async fn create_vault(
        &self,
        account: Uuid,
        header: &[u8],
        root_node_id: Uuid,
    ) -> Result<CreatedVault, VaultError> {
        if !self.admit("VaultService::create_vault", account, &[root_node_id]) {
            return Err(VaultError::NotFound);
        }
        self.inner.create_vault(account, header, root_node_id).await
    }

    async fn get_header(&self, account: Uuid, vault_id: Uuid) -> Result<VaultHeader, VaultError> {
        if !self.admit("VaultService::get_header", account, &[vault_id]) {
            return Err(VaultError::NotFound);
        }
        self.inner.get_header(account, vault_id).await
    }

    async fn replace_header(
        &self,
        account: Uuid,
        vault_id: Uuid,
        header: &[u8],
        if_match: &ETag,
    ) -> Result<ETag, VaultError> {
        if !self.admit("VaultService::replace_header", account, &[vault_id]) {
            return Err(VaultError::NotFound);
        }
        self.inner
            .replace_header(account, vault_id, header, if_match)
            .await
    }

    async fn report_unlock_failure(&self, account: Uuid, vault_id: Uuid) -> Result<(), VaultError> {
        if !self.admit("VaultService::report_unlock_failure", account, &[vault_id]) {
            return Err(VaultError::NotFound);
        }
        self.inner.report_unlock_failure(account, vault_id).await
    }
}
