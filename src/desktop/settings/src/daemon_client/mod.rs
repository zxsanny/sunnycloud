//! `DaemonClient`: the settings window's only connection to the daemon (E-25/c05).
//!
//! It speaks newline-delimited JSON-RPC 2.0 (`ipc_schema.md`, Transport) over the local
//! endpoint. The endpoint is checked to belong to the current user on every connect and
//! reconnect, before the first byte is sent, and no API skips that check. Every call ends
//! within the request timeout with a result or a [`ClientError`]. A lost or absent
//! connection is retried with a backoff until the client is dropped.
//!
//! Run it inside a tokio runtime: `spawn` starts a background task on the current one.

mod backoff;
mod endpoint;
mod error;
mod frames;
mod session;
mod typed;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use ipc_schema::{MethodCall, StatusEvent};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

pub use error::{ClientError, LocateError};
pub use frames::MAX_FRAME_BYTES;

/// No call waits longer than this (15 §10; matches the AC-16 apply budget).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Reconnect delays grow from `BACKOFF_MIN` to at most `BACKOFF_MAX` (15 §10).
pub const BACKOFF_MIN: Duration = Duration::from_millis(250);
pub const BACKOFF_MAX: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Unix socket path, or Windows pipe name.
    pub endpoint: PathBuf,
    pub request_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    pub max_frame_bytes: usize,
    /// The user the endpoint must belong to.
    #[cfg(unix)]
    pub owner_uid: u32,
}

impl ClientConfig {
    /// The production constants for `endpoint`, trusted for the current user.
    pub fn new(endpoint: PathBuf) -> Self {
        Self {
            endpoint,
            request_timeout: REQUEST_TIMEOUT,
            backoff_min: BACKOFF_MIN,
            backoff_max: BACKOFF_MAX,
            max_frame_bytes: MAX_FRAME_BYTES,
            #[cfg(unix)]
            owner_uid: endpoint::current_owner(),
        }
    }

    /// The daemon's endpoint for the current user: Linux
    /// `$XDG_RUNTIME_DIR/sunnycloud/daemon.sock`, Windows
    /// `\\.\pipe\sunnycloud-daemon-<user SID>` (the daemon's names, `ipc_schema.md` open
    /// point 1).
    pub fn for_current_user() -> Result<Self, LocateError> {
        Ok(Self::new(endpoint::default_endpoint()?))
    }
}

/// What the connection is doing now. `Down.attempt` counts failed attempts since the last
/// connection, so every attempt is a visible change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    /// `cause` is `DaemonNotRunning`, `EndpointUntrusted` or `Io`.
    Down {
        attempt: u32,
        cause: ClientError,
    },
}

type Reply = Result<Value, ClientError>;

enum Command {
    Request {
        id: i64,
        call: MethodCall,
        reply: oneshot::Sender<Reply>,
    },
    /// The caller stopped waiting; a late answer for `id` is dropped.
    Forget(i64),
}

#[derive(Clone)]
pub struct DaemonClient {
    commands: mpsc::UnboundedSender<Command>,
    next_id: Arc<AtomicI64>,
    state: watch::Receiver<ConnectionState>,
    request_timeout: Duration,
}

impl DaemonClient {
    /// Starts the connection task. The receiver gets every `StatusEvent` the daemon sends
    /// on the current connection, in the order sent; the task ends when the last client
    /// handle is dropped.
    pub fn spawn(config: ClientConfig) -> (Self, mpsc::UnboundedReceiver<StatusEvent>) {
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (events, event_rx) = mpsc::unbounded_channel();
        let (state_tx, state) = watch::channel(ConnectionState::Connecting);
        let client = Self {
            commands,
            next_id: Arc::new(AtomicI64::new(1)),
            state,
            request_timeout: config.request_timeout,
        };
        tokio::spawn(session::run(session::Actor::new(
            config, command_rx, state_tx, events,
        )));
        (client, event_rx)
    }

    pub fn connection(&self) -> watch::Receiver<ConnectionState> {
        self.state.clone()
    }

    /// One request and its answer, within the request timeout. A result that does not
    /// decode as `R` is a protocol violation.
    pub async fn call<R: DeserializeOwned>(&self, call: MethodCall) -> Result<R, ClientError> {
        let method = call.name();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Request { id, call, reply })
            .map_err(|_| ClientError::Closed)?;
        let value = match timeout(self.request_timeout, answer).await {
            Ok(Ok(reply)) => reply?,
            Ok(Err(_)) => return Err(ClientError::Closed),
            Err(_) => {
                // A closed channel means the task is gone and there is nothing to forget.
                if self.commands.send(Command::Forget(id)).is_err() {
                    return Err(ClientError::Closed);
                }
                return Err(ClientError::Timeout);
            }
        };
        serde_json::from_value(value).map_err(|error| {
            tracing::error!(method, kind = ?error.classify(), "protocol violation: result does not decode");
            ClientError::Protocol(format!("the result of {method} does not decode"))
        })
    }
}
