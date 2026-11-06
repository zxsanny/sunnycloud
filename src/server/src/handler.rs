use std::future::Future;
use std::pin::Pin;

use tokio_util::sync::CancellationToken;

use crate::commands::Command;
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::outcome::Outcome;
use crate::terminal::Terminal;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a handler gets: the parsed command, the lazy services, the token a SIGINT triggers, and the streams
/// (for progress lines on stderr).
pub struct Call<'a> {
    pub command: &'a Command,
    pub services: &'a ServiceContext,
    pub cancel: CancellationToken,
    pub terminal: &'a Terminal,
}

/// Runs a command: one business call on its owning port, mapped to an [`Outcome`] or a [`CliError`]. The
/// guard, the writers, the exit code and the log line are not its business.
pub trait Handler: Send + Sync {
    fn run<'a>(&'a self, call: Call<'a>) -> BoxFuture<'a, Result<Outcome, CliError>>;
}
