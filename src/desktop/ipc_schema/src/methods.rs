use serde_json::{Map, Value};

use crate::ids::PairId;
use crate::params::{
    ListEntriesParams, ListServerFoldersParams, LoginStartParams, NoParams, PairIdParams,
    ResolveHoldParams, SetServerParams, UpdatePairParams,
};
use crate::rpc::RpcError;
use crate::types::{
    DaemonStatus, EscapedPage, FolderPage, LoginPrompt, LoginState, LogoutOutcome, PairSpec,
    PairView, Settings, SettingsPatch, SkippedPage,
};

/// One row of the method table: wire name and the type names of params and result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodInfo {
    pub name: &'static str,
    pub params: &'static str,
    pub result: &'static str,
}

/// Single source of the method table: it defines the typed call enum, the wire names, the
/// decoding and `METHODS`, so none of them can drift apart.
macro_rules! methods {
    ($($variant:ident $name:literal ($params:ty) -> $result:ty;)+) => {
        /// One decoded request: the method and its typed params.
        #[derive(Debug, Clone, PartialEq)]
        pub enum MethodCall {
            $($variant($params),)+
        }

        pub const METHODS: &[MethodInfo] = &[
            $(MethodInfo {
                name: $name,
                params: stringify!($params),
                result: stringify!($result),
            },)+
        ];

        impl MethodCall {
            pub fn name(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $name,)+
                }
            }

            pub fn params_value(&self) -> Result<Value, serde_json::Error> {
                match self {
                    $(Self::$variant(params) => serde_json::to_value(params),)+
                }
            }

            /// Checks that `result` decodes to this method's result type.
            pub fn validate_result(&self, result: Value) -> Result<(), serde_json::Error> {
                match self {
                    $(Self::$variant(_) => serde_json::from_value::<$result>(result).map(drop),)+
                }
            }

            /// Unknown method: -32601. Params that do not decode: -32602. Absent params
            /// read as an empty object.
            pub fn decode(method: &str, params: Option<Value>) -> Result<Self, RpcError> {
                let params = params.unwrap_or_else(|| Value::Object(Map::new()));
                match method {
                    $($name => serde_json::from_value::<$params>(params)
                        .map(Self::$variant)
                        .map_err(RpcError::invalid_params),)+
                    other => Err(RpcError::method_not_found(other)),
                }
            }
        }
    };
}

methods! {
    Status "status" (NoParams) -> DaemonStatus;
    SetServer "set_server" (SetServerParams) -> ();
    ListServerFolders "list_server_folders" (ListServerFoldersParams) -> FolderPage;
    ListPairs "list_pairs" (NoParams) -> Vec<PairView>;
    AddPair "add_pair" (PairSpec) -> PairId;
    UpdatePair "update_pair" (UpdatePairParams) -> ();
    RemovePair "remove_pair" (PairIdParams) -> ();
    Pause "pause" (PairIdParams) -> ();
    Resume "resume" (PairIdParams) -> ();
    ListSkipped "list_skipped" (ListEntriesParams) -> SkippedPage;
    ListEscaped "list_escaped" (ListEntriesParams) -> EscapedPage;
    GetSettings "get_settings" (NoParams) -> Settings;
    SetSettings "set_settings" (SettingsPatch) -> Settings;
    ResolveHold "resolve_hold" (ResolveHoldParams) -> ();
    LoginStart "login_start" (LoginStartParams) -> LoginPrompt;
    LoginStatus "login_status" (NoParams) -> LoginState;
    Logout "logout" (NoParams) -> LogoutOutcome;
    Subscribe "subscribe" (NoParams) -> ();
}
