use std::collections::HashMap;

use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountInfo, AccountLifecycleHook,
    AccountState, CheckFuture, Drift, EraseProgress, HealthContributor, HealthItem, LifecycleError,
    ListingPage, LiveFileLookup, NodeRef, NodeSelector, NodeView, Page, QuotaAdmin,
    QuotaAdminError, QuotaView, RevisionMatch, RevisionView, Role, TreeReader, TreeReaderError,
    async_trait,
};
use sc_db::DbTx;
use sc_ids::FileHash;
use uuid::Uuid;

const NOT_CALLED: &str = "the skeleton does not call this port";

pub struct AccountAdminDouble;

#[async_trait]
impl AccountAdmin for AccountAdminDouble {
    async fn list_accounts(
        &self,
        _filter: AccountFilter,
        _page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn lock_active_admins(&self, _tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn set_role(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _role: Role,
    ) -> Result<(), AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn set_state(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _state: AccountState,
    ) -> Result<(), AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn begin_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn finalize_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        Err(AccountAdminError::Unexpected(NOT_CALLED.into()))
    }
}

pub struct QuotaAdminDouble;

#[async_trait]
impl QuotaAdmin for QuotaAdminDouble {
    async fn view(&self, _account: Uuid) -> Result<QuotaView, QuotaAdminError> {
        Err(QuotaAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn views(&self, _accounts: &[Uuid]) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError> {
        Err(QuotaAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn set_limit(&self, _account: Uuid, _limit: Option<i64>) -> Result<(), QuotaAdminError> {
        Err(QuotaAdminError::Unexpected(NOT_CALLED.into()))
    }

    async fn recompute(
        &self,
        _account: Uuid,
        _live_bytes: i64,
        _reserved_bytes: i64,
        _repair: bool,
    ) -> Result<Drift, QuotaAdminError> {
        Err(QuotaAdminError::Unexpected(NOT_CALLED.into()))
    }
}

pub struct TreeReaderDouble;

#[async_trait]
impl TreeReader for TreeReaderDouble {
    async fn get_node(
        &self,
        _account: Uuid,
        _selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn list_children(
        &self,
        _account: Uuid,
        _folder_id: Uuid,
        _after_name_key: Option<&str>,
        _limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn find_live_node(
        &self,
        _account: Uuid,
        _parent_id: Uuid,
        _name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn resolve_live_file(
        &self,
        _account: Uuid,
        _node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn root_node(&self, _account: Uuid) -> Result<NodeRef, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn get_revision(
        &self,
        _account: Uuid,
        _revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn is_current_or_retained(
        &self,
        _account: Uuid,
        _revision_id: Uuid,
    ) -> Result<bool, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn describe_blobs(
        &self,
        _blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }

    async fn lookup_revisions(
        &self,
        _account: Uuid,
        _hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError> {
        Err(TreeReaderError::Unexpected(NOT_CALLED.into()))
    }
}

/// A hook that knows which component it stands for, so a failing order assertion names it.
pub struct HookDouble(pub &'static str);

#[async_trait]
impl AccountLifecycleHook for HookDouble {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Err(LifecycleError::Unexpected(self.0.into()))
    }

    async fn erase_batch(
        &self,
        _account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        Err(LifecycleError::Unexpected(self.0.into()))
    }
}

pub struct HealthDouble(pub &'static str);

impl HealthContributor for HealthDouble {
    fn name(&self) -> &str {
        self.0
    }

    fn check(&self) -> CheckFuture<'_, HealthItem> {
        Box::pin(async { HealthItem::ok() })
    }
}
