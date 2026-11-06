use std::time::Duration;

use clap::Args;

use super::parse_duration;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Run the checks that need a stopped API (before a restore or an upgrade).
    #[arg(long)]
    pub pre_flight: bool,
    /// Longest accepted time since the last backup, for example 24h.
    #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
    pub backup_interval: Option<Duration>,
    /// Record that the operator confirms a recent backup.
    #[arg(long)]
    pub attest_backup: bool,
}
