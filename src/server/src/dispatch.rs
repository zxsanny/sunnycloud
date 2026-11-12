use sc_error::ErrorCode;
use sunnycloud_http_api::current_document;

use crate::command_config;
use crate::commands::Command;
use crate::error::CliError;
use crate::exit::Exit;
use crate::handler::{BoxFuture, Call, Handler};
use crate::healthcheck;
use crate::migration;
use crate::outcome::Outcome;

/// The binary's handler. Each command task replaces its `NotImplemented` arm with a call into its own module.
pub struct Commands;

impl Handler for Commands {
    fn run<'a>(&'a self, call: Call<'a>) -> BoxFuture<'a, Result<Outcome, CliError>> {
        Box::pin(async move {
            command_config::check(call.command, call.services).await?;
            match call.command {
                Command::Version => Ok(version()),
                Command::Openapi => openapi(),
                Command::Healthcheck => healthcheck::run(healthcheck::OPS_ADDR, &call.cancel).await,
                Command::Migrate(args) => migration::run(call.command, args, call.services).await,
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

/// The document of this build, byte for byte as `GET /api/v1/openapi.json` serves it. It comes from the route
/// registry alone, so no service, database or key is built.
fn openapi() -> Result<Outcome, CliError> {
    let document = current_document();
    let text = std::str::from_utf8(document.as_bytes()).map_err(|error| CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: format!("the OpenAPI document is not UTF-8: {error}"),
        exit: Exit::Failed,
    })?;
    Ok(Outcome::success().document(text))
}
