//! The accept loop: one connection at a time; the next is not accepted until the previous one
//! has closed and its job has stopped, so a second connection waits in the kernel backlog.

use std::io::ErrorKind;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use sc_observability::{log_info, log_warn};

use crate::config::RunnerConfig;
use crate::connection::{ConnectionEnd, TICK, serve_connection};
use crate::error::RunnerError;
use crate::job::JobHandler;
use crate::watchdog::Heartbeat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The stop flag was set.
    Stopped,
    /// The client broke the protocol and got the typed reply; the process exits (10 section 3).
    ProtocolViolation,
}

/// Runs the accept loop. Every pass of it, idle or serving, bumps `heartbeat`.
pub fn serve<H: JobHandler>(
    listener: &UnixListener,
    handler: &Arc<H>,
    config: &RunnerConfig,
    stop: &AtomicBool,
    heartbeat: &Heartbeat,
) -> Result<Exit, RunnerError> {
    while !stop.load(Ordering::Acquire) {
        heartbeat.beat();
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(RunnerError::io("make connection blocking"))?;
                log_info!("connection accepted");
                match serve_connection(stream, handler, config, heartbeat) {
                    ConnectionEnd::Closed => log_info!("connection closed"),
                    ConnectionEnd::Violation => return Ok(Exit::ProtocolViolation),
                    ConnectionEnd::Fatal(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => thread::sleep(TICK),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::ConnectionAborted => {
                log_warn!("connection aborted before accept", err = &error);
            }
            Err(source) => {
                return Err(RunnerError::Io {
                    what: "accept connection",
                    source,
                });
            }
        }
    }
    Ok(Exit::Stopped)
}
