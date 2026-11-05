use std::sync::Arc;

use crate::error::ControlResult;
use crate::ids::PairId;
use crate::methods::MethodCall;
use crate::params::{
    ListEntriesParams, ListServerFoldersParams, LoginStartParams, PairIdParams, ResolveHoldParams,
    SetServerParams, UpdatePairParams,
};
use crate::rpc::{RequestId, Response};
use crate::types::{
    DaemonStatus, EscapedPage, FolderPage, LoginPrompt, LoginState, LogoutOutcome, PairSpec,
    PairView, Settings, SettingsPatch, SkippedPage, StatusEvent,
};

/// Receives the `StatusEvent`s of one subscribed connection. `send` must not block.
pub trait StatusSink: Send + Sync {
    fn send(&self, event: StatusEvent);
}

/// The control surface of the daemon, called by the IPC server and the tray.
/// Behaviour: `contracts/14_desktop_daemon/daemon_control.md`.
pub trait DaemonControl {
    fn status(&self) -> impl Future<Output = ControlResult<DaemonStatus>> + Send;

    fn set_server(&self, params: SetServerParams)
    -> impl Future<Output = ControlResult<()>> + Send;

    fn list_server_folders(
        &self,
        params: ListServerFoldersParams,
    ) -> impl Future<Output = ControlResult<FolderPage>> + Send;

    fn list_pairs(&self) -> impl Future<Output = ControlResult<Vec<PairView>>> + Send;

    fn add_pair(&self, spec: PairSpec) -> impl Future<Output = ControlResult<PairId>> + Send;

    fn update_pair(
        &self,
        params: UpdatePairParams,
    ) -> impl Future<Output = ControlResult<()>> + Send;

    fn remove_pair(&self, params: PairIdParams) -> impl Future<Output = ControlResult<()>> + Send;

    fn pause(&self, params: PairIdParams) -> impl Future<Output = ControlResult<()>> + Send;

    fn resume(&self, params: PairIdParams) -> impl Future<Output = ControlResult<()>> + Send;

    fn list_skipped(
        &self,
        params: ListEntriesParams,
    ) -> impl Future<Output = ControlResult<SkippedPage>> + Send;

    fn list_escaped(
        &self,
        params: ListEntriesParams,
    ) -> impl Future<Output = ControlResult<EscapedPage>> + Send;

    fn get_settings(&self) -> impl Future<Output = ControlResult<Settings>> + Send;

    fn set_settings(
        &self,
        patch: SettingsPatch,
    ) -> impl Future<Output = ControlResult<Settings>> + Send;

    fn resolve_hold(
        &self,
        params: ResolveHoldParams,
    ) -> impl Future<Output = ControlResult<()>> + Send;

    fn login_start(
        &self,
        params: LoginStartParams,
    ) -> impl Future<Output = ControlResult<LoginPrompt>> + Send;

    fn login_status(&self) -> impl Future<Output = ControlResult<LoginState>> + Send;

    fn logout(&self) -> impl Future<Output = ControlResult<LogoutOutcome>> + Send;

    /// Registers `sink`; from then on every change of any `DaemonStatus` field sends one
    /// `StatusEvent` with the full new status.
    fn subscribe(
        &self,
        sink: Arc<dyn StatusSink>,
    ) -> impl Future<Output = ControlResult<()>> + Send;
}

/// Routes one decoded call to the trait and builds the response. The exhaustive match ties
/// the trait to the method table: a method added to the table does not compile until it
/// is routed here.
pub async fn dispatch<D: DaemonControl>(
    daemon: &D,
    sink: &Arc<dyn StatusSink>,
    id: RequestId,
    call: MethodCall,
) -> Response {
    match call {
        MethodCall::Status(_) => Response::from_outcome(id, daemon.status().await),
        MethodCall::SetServer(p) => Response::from_outcome(id, daemon.set_server(p).await),
        MethodCall::ListServerFolders(p) => {
            Response::from_outcome(id, daemon.list_server_folders(p).await)
        }
        MethodCall::ListPairs(_) => Response::from_outcome(id, daemon.list_pairs().await),
        MethodCall::AddPair(p) => Response::from_outcome(id, daemon.add_pair(p).await),
        MethodCall::UpdatePair(p) => Response::from_outcome(id, daemon.update_pair(p).await),
        MethodCall::RemovePair(p) => Response::from_outcome(id, daemon.remove_pair(p).await),
        MethodCall::Pause(p) => Response::from_outcome(id, daemon.pause(p).await),
        MethodCall::Resume(p) => Response::from_outcome(id, daemon.resume(p).await),
        MethodCall::ListSkipped(p) => Response::from_outcome(id, daemon.list_skipped(p).await),
        MethodCall::ListEscaped(p) => Response::from_outcome(id, daemon.list_escaped(p).await),
        MethodCall::GetSettings(_) => Response::from_outcome(id, daemon.get_settings().await),
        MethodCall::SetSettings(p) => Response::from_outcome(id, daemon.set_settings(p).await),
        MethodCall::ResolveHold(p) => Response::from_outcome(id, daemon.resolve_hold(p).await),
        MethodCall::LoginStart(p) => Response::from_outcome(id, daemon.login_start(p).await),
        MethodCall::LoginStatus(_) => Response::from_outcome(id, daemon.login_status().await),
        MethodCall::Logout(_) => Response::from_outcome(id, daemon.logout().await),
        MethodCall::Subscribe(_) => {
            Response::from_outcome(id, daemon.subscribe(Arc::clone(sink)).await)
        }
    }
}
