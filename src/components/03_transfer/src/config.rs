use std::time::Duration;

use sc_config::{KeyProblem, Reason, Settings};
use sc_ids::BLOCK_SIZE;

/// `upload_sessions.block_count` is a PostgreSQL `integer`, so a declared size may span at most `i32::MAX` blocks.
const MAX_DECLARED_SIZE_BOUND: u64 = i32::MAX as u64 * BLOCK_SIZE as u64;

/// The transfer section of the key table, typed. Defaults and ranges come from `sc_config`. The 2 MiB block size
/// is a protocol constant ([`BLOCK_SIZE`]), not a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferConfig {
    pub upload_session_ttl: Duration,
    pub max_declared_size: u64,
    pub max_open_sessions: u64,
    pub max_staged: u64,
    pub checkpoint_bytes: u64,
    pub checkpoint_interval: Duration,
    pub max_lanes: u64,
    /// In-flight blocks of one account, large and staged. Not a key of the table: `max_open_sessions` times
    /// `max_lanes`, 32 by default.
    pub max_inflight_blocks_per_account: u64,
    pub batch_max_entries: u64,
    pub commit_receipt_ttl: Duration,
    pub max_buffered_block_bytes: u64,
    pub session_reap_interval: Duration,
    pub receipt_janitor_interval: Duration,
}

impl TransferConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        TransferConfig {
            upload_session_ttl: settings.upload_session_ttl.get(),
            max_declared_size: settings.max_declared_size.bytes(),
            max_open_sessions: settings.max_open_sessions,
            max_staged: settings.max_staged,
            checkpoint_bytes: settings.checkpoint_bytes.bytes(),
            checkpoint_interval: settings.checkpoint_interval.get(),
            max_lanes: settings.max_lanes,
            max_inflight_blocks_per_account: settings
                .max_open_sessions
                .saturating_mul(settings.max_lanes),
            batch_max_entries: settings.batch_max_entries,
            commit_receipt_ttl: settings.commit_receipt_ttl.get(),
            max_buffered_block_bytes: settings.max_buffered_block_bytes.bytes(),
            session_reap_interval: settings.session_reap_interval.get(),
            receipt_janitor_interval: settings.receipt_janitor_interval.get(),
        }
    }

    /// Start validation: every problem, each naming its key. A zero count or interval would refuse every
    /// request or spin a loop, a buffer smaller than one block could never admit a block, and a declared size
    /// beyond the `integer` block count of the session row could not be stored.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        for (key, value) in [
            ("upload_session_ttl", self.upload_session_ttl),
            ("checkpoint_interval", self.checkpoint_interval),
            ("commit_receipt_ttl", self.commit_receipt_ttl),
            ("session_reap_interval", self.session_reap_interval),
            ("receipt_janitor_interval", self.receipt_janitor_interval),
        ] {
            if value.is_zero() {
                problems.push(out_of_range(key, "1ms", "no ceiling"));
            }
        }
        for (key, value) in [
            ("max_open_sessions", self.max_open_sessions),
            ("max_staged", self.max_staged),
            ("checkpoint_bytes", self.checkpoint_bytes),
            ("max_lanes", self.max_lanes),
            ("batch_max_entries", self.batch_max_entries),
        ] {
            if value == 0 {
                problems.push(out_of_range(key, "1", "no ceiling"));
            }
        }
        if !(1..=MAX_DECLARED_SIZE_BOUND).contains(&self.max_declared_size) {
            problems.push(out_of_range(
                "max_declared_size",
                "1B",
                &format!("{}B", MAX_DECLARED_SIZE_BOUND),
            ));
        }
        if self.max_buffered_block_bytes < BLOCK_SIZE as u64 {
            problems.push(out_of_range(
                "max_buffered_block_bytes",
                "2MiB",
                "no ceiling",
            ));
        }
        problems
    }
}

fn out_of_range(key: &str, min: &str, max: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: max.to_owned(),
        },
    )
}
