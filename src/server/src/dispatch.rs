use sc_error::ErrorCode;
use sunnycloud_http_api::current_document;

use crate::audit_commands;
use crate::command_config;
use crate::commands::{AdminCommand, AuditCommand, Command, ScrubCommand, UserCommand};
use crate::doctor_command;
use crate::error::CliError;
use crate::exit::Exit;
use crate::handler::{BoxFuture, Call, Handler};
use crate::healthcheck;
use crate::key_commands;
use crate::maintenance_commands;
use crate::media_commands;
use crate::migration;
use crate::outcome::Outcome;
use crate::scrub_commands;
use crate::serve;
use crate::user_commands;

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
                Command::Serve => serve::run(call.services, call.cancel).await,
                Command::Doctor(args) => doctor_command::run(args, call.services).await,
                Command::KeyCheck => key_commands::key_check(call.services).await,
                Command::Admin {
                    command: AdminCommand::KeycheckInit,
                } => key_commands::keycheck_init(call.services).await,
                Command::Admin {
                    command: AdminCommand::Rewrap(args),
                } => key_commands::rewrap(args, &call).await,
                Command::Admin {
                    command: AdminCommand::MediaRequeue { account, kind },
                } => {
                    media_commands::requeue(account.as_deref(), kind.as_deref(), call.services)
                        .await
                }
                Command::Admin {
                    command: AdminCommand::EpochBump(_),
                } => maintenance_commands::epoch_bump(call.services).await,
                Command::Admin {
                    command: AdminCommand::Restore(args),
                } => maintenance_commands::restore(args, call.services).await,
                Command::Admin {
                    command:
                        AdminCommand::User {
                            command: UserCommand::List,
                        },
                } => user_commands::list(call.services).await,
                Command::Admin {
                    command:
                        AdminCommand::User {
                            command: UserCommand::SetQuota { account, size },
                        },
                } => user_commands::set_quota(account, *size, call.services).await,
                Command::Admin {
                    command:
                        AdminCommand::Audit {
                            command: AuditCommand::Export { since },
                        },
                } => audit_commands::export(since, &call).await,
                Command::Admin {
                    command: AdminCommand::GcRun,
                } => maintenance_commands::gc_run(call.services).await,
                Command::Admin {
                    command: AdminCommand::MediaStats,
                } => media_commands::stats(call.services).await,
                Command::Scrub {
                    command: ScrubCommand::Run { full, rate },
                } => scrub_commands::run(*full, *rate, &call).await,
                Command::Scrub {
                    command: ScrubCommand::Status,
                } => scrub_commands::status(call.services).await,
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
