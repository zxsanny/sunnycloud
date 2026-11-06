use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ScrubCommand {
    /// Verify blobs and report mismatches and missing blobs.
    Run {
        /// Verify every blob, not only the ones not scrubbed recently.
        #[arg(long)]
        full: bool,
        /// Read rate limit.
        #[arg(long, value_name = "MiB/s", value_parser = clap::value_parser!(u32).range(1..))]
        rate: Option<u32>,
    },
    /// Show the oldest scrub time, the unscrubbed count and open findings.
    Status,
}

impl ScrubCommand {
    pub(super) fn route(&self) -> &'static str {
        match self {
            ScrubCommand::Run { .. } => "/cli/scrub-run",
            ScrubCommand::Status => "/cli/scrub-status",
        }
    }
}
