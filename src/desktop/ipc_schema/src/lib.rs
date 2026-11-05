//! Schema shared by the desktop daemon (14) and its local clients (15, the tray): the
//! `DaemonControl` trait, every parameter, result and error type, the JSON-RPC 2.0
//! envelopes and the one-object-per-line framing. Contract:
//! `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md`.
//!
//! No type reachable from a method result or a `StatusEvent` can carry a token, secret or
//! credential; `tests/schema_walk.rs` walks the type definitions to prove it.

mod control;
mod error;
mod framing;
mod ids;
mod methods;
mod params;
mod rpc;
mod types;

pub use control::{DaemonControl, StatusSink, dispatch};
pub use error::{ControlError, ControlResult};
pub use framing::{Call, LineBuffer, decode_call, encode_line};
pub use ids::{InvalidNodeId, NodeId, PageToken, PairId};
pub use methods::{METHODS, MethodCall, MethodInfo};
pub use params::{
    ListEntriesParams, ListServerFoldersParams, LoginStartParams, NoParams, PairIdParams,
    ResolveHoldParams, SetServerParams, UpdatePairParams,
};
pub use rpc::{
    CONTROL_ERROR_CODE, CallError, INTERNAL_ERROR_CODE, INVALID_PARAMS_CODE, INVALID_REQUEST_CODE,
    METHOD_NOT_FOUND_CODE, Notification, Outcome, PARSE_ERROR_CODE, Request, RequestId, Response,
    RpcError, STATUS_EVENT_METHOD, ServerMessage, Version,
};
pub use types::{
    AccountView, DaemonStatus, EscapedEntry, EscapedPage, FolderEntry, FolderPage, HoldDecision,
    HoldKind, HoldView, LoginPrompt, LoginState, LogoutOutcome, PairPatch, PairSpec, PairState,
    PairTarget, PairView, Settings, SettingsPatch, SkipReason, SkippedEntry, SkippedPage,
    StatusEvent, UpdateView,
};
