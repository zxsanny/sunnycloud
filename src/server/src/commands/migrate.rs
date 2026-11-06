use clap::{Args, Subcommand};

use super::{ConfirmArgs, Confirmation};

#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// Without a subcommand, `run` is meant.
    #[command(subcommand)]
    pub action: Option<MigrateAction>,
}

#[derive(Debug, Subcommand)]
pub enum MigrateAction {
    /// Apply every pending migration.
    Run,
    /// List applied, pending, dirty and unknown-newer migrations.
    Status,
    /// Revert the most recent migrations.
    Revert {
        /// How many migrations to revert.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        steps: u32,
        #[command(flatten)]
        confirm: ConfirmArgs,
    },
}

pub(super) fn route(args: &MigrateArgs) -> &'static str {
    match args.action {
        None | Some(MigrateAction::Run) => "/cli/migrate",
        Some(MigrateAction::Status) => "/cli/migrate-status",
        Some(MigrateAction::Revert { .. }) => "/cli/migrate-revert",
    }
}

pub(super) fn confirmation(args: &MigrateArgs) -> Option<Confirmation> {
    match &args.action {
        Some(MigrateAction::Revert { steps, confirm }) => Some(Confirmation {
            yes: confirm.yes,
            action: format!("Revert the last {steps} database migration(s)"),
        }),
        _ => None,
    }
}
