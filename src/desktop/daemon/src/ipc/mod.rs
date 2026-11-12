//! Local IPC server: newline-delimited JSON-RPC 2.0 over an endpoint only the daemon's
//! user can open. Contract: `contracts/14_desktop_daemon/ipc_schema.md` (Transport).
//!
//! Endpoint names (proposal for `ipc_schema.md` open point 1):
//! - Linux: `$XDG_RUNTIME_DIR/sunnycloud/daemon.sock`, directory 0700, socket 0600.
//! - Windows: `\\.\pipe\sunnycloud-daemon-<user SID>`.
//!
//! Leftover socket rule (task risk 1): a socket file of the daemon's own user in the
//! daemon's own 0700 directory that refuses connections is a crashed daemon's leftover and
//! is replaced. Anything else at the path (another owner, not a socket, wrong mode, a
//! socket that answers) makes the daemon refuse the endpoint.

mod connection;
mod error;
mod lines;
mod server;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(test)]
pub(crate) mod tests;

pub use error::EndpointRefused;
pub use lines::MAX_LINE_BYTES;
pub use server::IpcServer;

#[cfg(unix)]
pub use unix::{start, start_in};
#[cfg(windows)]
pub use windows::start;
