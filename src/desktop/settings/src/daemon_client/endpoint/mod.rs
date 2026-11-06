//! Locating the daemon's endpoint and connecting to it only if it belongs to the current
//! user. The check lives in the one function the connection task calls, so no path to the
//! daemon skips it.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{Stream, connect, current_owner, default_endpoint};
#[cfg(windows)]
pub(super) use windows::{Stream, connect, default_endpoint};
