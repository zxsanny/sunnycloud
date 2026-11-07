use crate::command_config;
use crate::commands::Command;
use crate::error::CliError;
use crate::handler::{BoxFuture, Call, Handler};
use crate::outcome::Outcome;

/// The binary's handler. Each command task replaces its `NotImplemented` arm with a call into its own module.
pub struct Commands;

impl Handler for Commands {
    fn run<'a>(&'a self, call: Call<'a>) -> BoxFuture<'a, Result<Outcome, CliError>> {
        Box::pin(async move {
            command_config::check(call.command, call.services).await?;
            match call.command {
                Command::Version => Ok(version()),
                other => Err(CliError::NotImplemented(other.route())),
            }
        })
    }
}

fn version() -> Outcome {
    Outcome::success()
        .field("version", env!("CARGO_PKG_VERSION"))
        .field(
            "commit",
            option_env!("SUNNYCLOUD_GIT_COMMIT").unwrap_or("unknown"),
        )
}
