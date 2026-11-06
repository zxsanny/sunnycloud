//! The command tree: names, arguments and help text of `operator_command_surface.md`, one module per group.

mod admin;
mod doctor;
mod migrate;
mod scrub;

use std::time::Duration;

use clap::{Args, Parser, Subcommand};

pub use admin::{AdminCommand, AuditCommand, InviteCommand, RestoreArgs, RewrapArgs, UserCommand};
pub use doctor::DoctorArgs;
pub use migrate::{MigrateAction, MigrateArgs};
pub use scrub::ScrubCommand;

/// `sunnycloud [--json] <command>`.
#[derive(Debug, Parser)]
#[command(name = "sunnycloud", about = "Operate a sunnycloud server")]
pub struct Cli {
    /// Print exactly one JSON object `{result, code, data}` on stdout.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the API server.
    Serve,
    /// Apply, inspect or revert database migrations.
    Migrate(MigrateArgs),
    /// Check the installation and report findings.
    Doctor(DoctorArgs),
    /// Verify the master key file against the stored key check.
    KeyCheck,
    /// Administer accounts, invites, keys, restores and background work.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Verify stored blobs against their hashes.
    Scrub {
        #[command(subcommand)]
        command: ScrubCommand,
    },
    /// Probe the local readiness endpoint; exit 0 on 200.
    Healthcheck,
    /// Print the OpenAPI document.
    Openapi,
    /// Print the version and git commit.
    Version,
}

/// What a destructive command asks before it runs. `action` names the target.
#[derive(Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub yes: bool,
    pub action: String,
}

/// The `--yes` flag every destructive command accepts.
#[derive(Debug, Args)]
pub struct ConfirmArgs {
    /// Do not ask for confirmation.
    #[arg(long)]
    pub yes: bool,
}

impl Command {
    /// Static name of the command for the log line: `/cli/` plus the command words joined by `-`.
    pub fn route(&self) -> &'static str {
        match self {
            Command::Serve => "/cli/serve",
            Command::Migrate(args) => migrate::route(args),
            Command::Doctor(_) => "/cli/doctor",
            Command::KeyCheck => "/cli/key-check",
            Command::Admin { command } => command.route(),
            Command::Scrub { command } => command.route(),
            Command::Healthcheck => "/cli/healthcheck",
            Command::Openapi => "/cli/openapi",
            Command::Version => "/cli/version",
        }
    }

    pub fn confirmation(&self) -> Option<Confirmation> {
        match self {
            Command::Migrate(args) => migrate::confirmation(args),
            Command::Admin { command } => command.confirmation(),
            _ => None,
        }
    }
}

/// Value parser for duration arguments (`24h`, `30s`), the same syntax as the config keys.
pub(crate) fn parse_duration(text: &str) -> Result<Duration, String> {
    sc_config::parse_duration(text)
        .ok_or_else(|| format!("`{text}` is not a duration such as 30s, 15m or 24h"))
}

/// Value parser for size arguments (`10GiB`), the same syntax as the config keys.
pub(crate) fn parse_size(text: &str) -> Result<u64, String> {
    sc_config::parse_size(text)
        .ok_or_else(|| format!("`{text}` is not a size such as 512MiB or 10GiB"))
}
