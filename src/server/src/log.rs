use std::error::Error;
use std::io::{self, Write};
use std::time::Duration;

use sc_observability::{LineSink, log_error, log_info, log_warn};

use crate::exit::Exit;

/// Log lines go to stderr: stdout carries the result, and with `--json` it holds exactly one object.
pub struct StderrSink;

impl LineSink for StderrSink {
    fn write_line(&self, line: &str) -> io::Result<()> {
        let mut buffer = Vec::with_capacity(line.len() + 1);
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
        io::stderr().lock().write_all(&buffer)
    }
}

/// The one line per command. The command is its route (`/cli/...`), the exit code is in the message: the
/// logging schema has no `command` or `exit_code` field. Nothing from the arguments reaches it.
pub fn command_finished(
    route: &'static str,
    exit: Exit,
    elapsed: Duration,
    error: Option<&(dyn Error + 'static)>,
) {
    let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    match (exit, error) {
        (Exit::Success, _) => {
            log_info!(
                "cli command finished: exit 0",
                route = route,
                duration_ms = duration_ms
            )
        }
        (Exit::Warnings, _) => {
            log_warn!(
                "cli command finished: exit 1",
                route = route,
                duration_ms = duration_ms
            )
        }
        (Exit::Failed, Some(error)) => {
            log_error!(
                "cli command finished: exit 2",
                route = route,
                duration_ms = duration_ms,
                err = error
            )
        }
        (Exit::Failed, None) => {
            log_error!(
                "cli command finished: exit 2",
                route = route,
                duration_ms = duration_ms
            )
        }
        (Exit::Usage, Some(error)) => {
            log_warn!(
                "cli command finished: exit 3",
                route = route,
                duration_ms = duration_ms,
                err = error
            )
        }
        (Exit::Usage, None) => {
            log_warn!(
                "cli command finished: exit 3",
                route = route,
                duration_ms = duration_ms
            )
        }
    }
}
