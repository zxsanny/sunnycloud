//! One method per `DaemonControl` method; each is `DaemonClient::call` with the typed params
//! of `ipc_schema`.

use ipc_schema::{
    DaemonStatus, EscapedPage, FolderPage, HoldDecision, ListEntriesParams,
    ListServerFoldersParams, LoginPrompt, LoginState, LogoutOutcome, MethodCall, NoParams, NodeId,
    PageToken, PairId, PairIdParams, PairPatch, PairSpec, PairView, ResolveHoldParams,
    SetServerParams, Settings, SettingsPatch, SkippedPage, UpdatePairParams,
};

use super::{ClientError, DaemonClient};

impl DaemonClient {
    pub async fn status(&self) -> Result<DaemonStatus, ClientError> {
        self.call(MethodCall::Status(NoParams {})).await
    }

    pub async fn set_server(&self, url: String) -> Result<(), ClientError> {
        self.call(MethodCall::SetServer(SetServerParams { url }))
            .await
    }

    pub async fn list_server_folders(
        &self,
        parent: Option<NodeId>,
        page: Option<PageToken>,
    ) -> Result<FolderPage, ClientError> {
        self.call(MethodCall::ListServerFolders(ListServerFoldersParams {
            parent,
            page,
        }))
        .await
    }

    pub async fn list_pairs(&self) -> Result<Vec<PairView>, ClientError> {
        self.call(MethodCall::ListPairs(NoParams {})).await
    }

    pub async fn add_pair(&self, spec: PairSpec) -> Result<PairId, ClientError> {
        self.call(MethodCall::AddPair(spec)).await
    }

    pub async fn update_pair(&self, pair_id: PairId, patch: PairPatch) -> Result<(), ClientError> {
        self.call(MethodCall::UpdatePair(UpdatePairParams { pair_id, patch }))
            .await
    }

    pub async fn remove_pair(&self, pair_id: PairId) -> Result<(), ClientError> {
        self.call(MethodCall::RemovePair(PairIdParams { pair_id }))
            .await
    }

    pub async fn pause(&self, pair_id: PairId) -> Result<(), ClientError> {
        self.call(MethodCall::Pause(PairIdParams { pair_id })).await
    }

    pub async fn resume(&self, pair_id: PairId) -> Result<(), ClientError> {
        self.call(MethodCall::Resume(PairIdParams { pair_id }))
            .await
    }

    pub async fn list_skipped(
        &self,
        pair_id: Option<PairId>,
        page: Option<PageToken>,
    ) -> Result<SkippedPage, ClientError> {
        self.call(MethodCall::ListSkipped(ListEntriesParams { pair_id, page }))
            .await
    }

    pub async fn list_escaped(
        &self,
        pair_id: Option<PairId>,
        page: Option<PageToken>,
    ) -> Result<EscapedPage, ClientError> {
        self.call(MethodCall::ListEscaped(ListEntriesParams { pair_id, page }))
            .await
    }

    pub async fn get_settings(&self) -> Result<Settings, ClientError> {
        self.call(MethodCall::GetSettings(NoParams {})).await
    }

    pub async fn set_settings(&self, patch: SettingsPatch) -> Result<Settings, ClientError> {
        self.call(MethodCall::SetSettings(patch)).await
    }

    pub async fn resolve_hold(
        &self,
        pair_id: PairId,
        decision: HoldDecision,
    ) -> Result<(), ClientError> {
        self.call(MethodCall::ResolveHold(ResolveHoldParams {
            pair_id,
            decision,
        }))
        .await
    }

    pub async fn login_start(&self, device_name: String) -> Result<LoginPrompt, ClientError> {
        self.call(MethodCall::LoginStart(ipc_schema::LoginStartParams {
            device_name,
        }))
        .await
    }

    pub async fn login_status(&self) -> Result<LoginState, ClientError> {
        self.call(MethodCall::LoginStatus(NoParams {})).await
    }

    pub async fn logout(&self) -> Result<LogoutOutcome, ClientError> {
        self.call(MethodCall::Logout(NoParams {})).await
    }

    /// After this call succeeds the daemon sends `StatusEvent`s on this connection; they
    /// arrive on the receiver `DaemonClient::spawn` returned. A new connection must
    /// subscribe again.
    pub async fn subscribe(&self) -> Result<(), ClientError> {
        self.call(MethodCall::Subscribe(NoParams {})).await
    }
}
