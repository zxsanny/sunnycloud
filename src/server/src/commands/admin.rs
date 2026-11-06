use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use super::{ConfirmArgs, Confirmation, parse_duration, parse_size};

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Print the one-time setup token while no admin exists.
    SetupToken,
    /// Create an admin invite.
    Create {
        /// Free-text note stored with the invite.
        #[arg(long)]
        note: Option<String>,
        /// How long the invite stays valid, for example 24h.
        #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
        ttl: Option<Duration>,
    },
    /// Issue a fresh admin invite for an existing admin.
    Reset { account: String },
    /// Give an account the admin role.
    Promote { account: String },
    /// Remove the admin role from an account.
    Demote { account: String },
    /// Create, list or revoke invites.
    Invite {
        #[command(subcommand)]
        command: InviteCommand,
    },
    /// List, limit or delete accounts.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
    /// Seed development data (refused in production).
    DevSeed,
    /// Export audit rows.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    /// Bump the sync epoch after a restore; every client resyncs.
    EpochBump(ConfirmArgs),
    /// Restore a path in an account to its state at a time.
    Restore(RestoreArgs),
    /// Run one garbage-collection cycle.
    GcRun,
    /// Write the master key check.
    KeycheckInit,
    /// Re-encrypt blob keys under a new master key.
    Rewrap(RewrapArgs),
    /// Requeue dead media jobs.
    MediaRequeue {
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Show media queue depth per lane and status.
    MediaStats,
}

#[derive(Debug, Subcommand)]
pub enum InviteCommand {
    /// Create an invite; the token is printed once.
    Create {
        /// Free-text note stored with the invite.
        #[arg(long)]
        note: Option<String>,
    },
    /// List invites (never their tokens).
    List,
    /// Revoke an invite.
    Revoke { id: String },
}

#[derive(Debug, Subcommand)]
pub enum UserCommand {
    /// List accounts with role, state, last login and usage.
    List,
    /// Set the storage limit of an account.
    SetQuota {
        account: String,
        #[arg(value_name = "SIZE", value_parser = parse_size)]
        size: u64,
    },
    /// Delete an account and all its data.
    Delete {
        account: String,
        #[command(flatten)]
        confirm: ConfirmArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Print audit rows (ids only) as JSON lines.
    Export {
        /// Start time (RFC 3339 UTC).
        #[arg(long, value_name = "TIME")]
        since: String,
    },
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    #[arg(long)]
    pub account: String,
    /// Path inside the account.
    #[arg(long)]
    pub path: String,
    /// Restore the revision current at this time (RFC 3339 UTC).
    #[arg(long, value_name = "TIME")]
    pub at: String,
    /// Report what would be restored without changing anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct RewrapArgs {
    /// File holding the new master key.
    #[arg(long, value_name = "PATH")]
    pub new_key_file: PathBuf,
    #[command(flatten)]
    pub confirm: ConfirmArgs,
}

impl AdminCommand {
    pub(super) fn route(&self) -> &'static str {
        match self {
            AdminCommand::SetupToken => "/cli/admin-setup-token",
            AdminCommand::Create { .. } => "/cli/admin-create",
            AdminCommand::Reset { .. } => "/cli/admin-reset",
            AdminCommand::Promote { .. } => "/cli/admin-promote",
            AdminCommand::Demote { .. } => "/cli/admin-demote",
            AdminCommand::Invite { command } => match command {
                InviteCommand::Create { .. } => "/cli/admin-invite-create",
                InviteCommand::List => "/cli/admin-invite-list",
                InviteCommand::Revoke { .. } => "/cli/admin-invite-revoke",
            },
            AdminCommand::User { command } => match command {
                UserCommand::List => "/cli/admin-user-list",
                UserCommand::SetQuota { .. } => "/cli/admin-user-set-quota",
                UserCommand::Delete { .. } => "/cli/admin-user-delete",
            },
            AdminCommand::DevSeed => "/cli/admin-dev-seed",
            AdminCommand::Audit { .. } => "/cli/admin-audit-export",
            AdminCommand::EpochBump(_) => "/cli/admin-epoch-bump",
            AdminCommand::Restore(_) => "/cli/admin-restore",
            AdminCommand::GcRun => "/cli/admin-gc-run",
            AdminCommand::KeycheckInit => "/cli/admin-keycheck-init",
            AdminCommand::Rewrap(_) => "/cli/admin-rewrap",
            AdminCommand::MediaRequeue { .. } => "/cli/admin-media-requeue",
            AdminCommand::MediaStats => "/cli/admin-media-stats",
        }
    }

    pub(super) fn confirmation(&self) -> Option<Confirmation> {
        match self {
            AdminCommand::User {
                command: UserCommand::Delete { account, confirm },
            } => Some(Confirmation {
                yes: confirm.yes,
                action: format!("Delete account {account} and all its data"),
            }),
            AdminCommand::EpochBump(confirm) => Some(Confirmation {
                yes: confirm.yes,
                action: "Bump the sync epoch (every client resyncs)".to_owned(),
            }),
            AdminCommand::Rewrap(args) => Some(Confirmation {
                yes: args.confirm.yes,
                action: "Re-encrypt every blob key under the new master key".to_owned(),
            }),
            _ => None,
        }
    }
}
