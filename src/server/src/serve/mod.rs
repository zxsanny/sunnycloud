//! `serve` (E-22/c05): the start in its documented order, serving until SIGTERM or SIGINT, and the shutdown in its
//! documented order. SIGTERM ends with exit 0, SIGINT with 130; a refusal at start or a failed shutdown step
//! with exit 2. On an install without accounts the start ends by printing the setup token (`setup`).

mod http;
mod setup;
mod start;
mod stop;

use sc_error::ErrorCode;
use sc_observability::log_info;
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::{Class, Outcome};
use crate::output::Mode;
use crate::terminal::Terminal;

/// The signal that ended the serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Terminate,
    Interrupt,
}

impl Stop {
    fn class(self) -> Class {
        match self {
            Stop::Terminate => Class::Success,
            Stop::Interrupt => Class::Interrupted,
        }
    }
}

/// `interrupt` is the token the CLI's SIGINT watch triggers.
pub(crate) async fn run(
    services: &ServiceContext,
    interrupt: CancellationToken,
    terminal: &Terminal,
    mode: Mode,
) -> Result<Outcome, CliError> {
    // Installed first, so a SIGTERM during the start is held for the serving instead of killing the process.
    let mut terminate = signal(SignalKind::terminate()).map_err(|error| CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: format!("cannot install the SIGTERM handler: {error}"),
        exit: Exit::Failed,
    })?;
    let started = start::start(services).await?;
    setup::announce(services, terminal, mode).await?;
    let mut stopped_by = None;
    log_info!("serve start: ready");
    started
        .server
        .serve(async {
            stopped_by = Some(tokio::select! {
                _ = terminate.recv() => Stop::Terminate,
                () = interrupt.cancelled() => Stop::Interrupt,
            });
        })
        .await
        .map_err(|error| CliError::Operator {
            code: ErrorCode::Internal.as_str(),
            detail: format!("http server: {error}"),
            exit: Exit::Failed,
        })?;
    stop::stop(services, &started.scheduler).await?;
    let stopped_by = stopped_by.ok_or_else(|| CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: "the http server returned before its shutdown signal".to_owned(),
        exit: Exit::Failed,
    })?;
    Ok(Outcome::new(stopped_by.class()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigterm_exits_0_and_sigint_exits_130() {
        // Act
        let terminate = Outcome::new(Stop::Terminate.class()).exit().code();
        let interrupt = Outcome::new(Stop::Interrupt.class()).exit().code();

        // Assert
        assert_eq!((terminate, interrupt), (0, 130));
    }
}
