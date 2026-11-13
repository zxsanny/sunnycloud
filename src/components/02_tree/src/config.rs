use std::time::Duration;

use sc_config::{KeyProblem, Reason, Settings};

/// `purge_batch_size` when the key is unset; the key table has no default for it.
pub const DEFAULT_PURGE_BATCH_SIZE: u64 = 5000;
/// `tree_maintenance_interval` when the key is unset; the key table has no default for it.
pub const DEFAULT_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);
/// The only accepted value of `conflict_copy_time_zone`.
const CONFLICT_COPY_TIME_ZONE: &str = "UTC";

/// The tree section of the key table, typed. Defaults and the production range of `tombstone_horizon` come from
/// `sc_config`; the two keys that table leaves unset take the defaults above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeConfig {
    pub tombstone_horizon: Duration,
    pub listing_default_limit: u64,
    /// No default exists for this key; `None` means no ceiling beyond the default limit is configured.
    pub listing_max_limit: u64,
    pub cascade_batch_size: u64,
    pub purge_batch_size: u64,
    pub tree_maintenance_interval: Duration,
    pub conflict_copy_time_zone: String,
}

impl TreeConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        TreeConfig {
            tombstone_horizon: settings.tombstone_horizon.get(),
            listing_default_limit: settings.listing_default_limit,
            listing_max_limit: settings.listing_max_limit,
            cascade_batch_size: settings.cascade_batch_size,
            purge_batch_size: settings
                .purge_batch_size
                .unwrap_or(DEFAULT_PURGE_BATCH_SIZE),
            tree_maintenance_interval: settings
                .tree_maintenance_interval
                .map_or(DEFAULT_MAINTENANCE_INTERVAL, |interval| interval.get()),
            conflict_copy_time_zone: settings.conflict_copy_time_zone.clone(),
        }
    }

    /// Start validation: every problem, each naming its key. A zero batch or limit would stall a loop or return
    /// no rows, a zero interval would spin, and a maximum below the default limit would make the default
    /// unreachable.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        for (key, value) in [
            ("listing_default_limit", self.listing_default_limit),
            ("cascade_batch_size", self.cascade_batch_size),
            ("purge_batch_size", self.purge_batch_size),
        ] {
            if value == 0 {
                problems.push(at_least(key, "1"));
            }
        }
        if self.listing_max_limit < self.listing_default_limit.max(1) {
            let floor = self.listing_default_limit.max(1).to_string();
            problems.push(at_least("listing_max_limit", &floor));
        }
        if self.tree_maintenance_interval.is_zero() {
            problems.push(at_least("tree_maintenance_interval", "1ms"));
        }
        // Conflict-copy names are formatted in UTC only (owner decision 2026-11-13): another zone would be
        // accepted and silently ignored, so it is refused at start.
        match self.conflict_copy_time_zone.trim() {
            "" => problems.push(KeyProblem::new(
                "conflict_copy_time_zone",
                Reason::MissingRequired,
            )),
            CONFLICT_COPY_TIME_ZONE => {}
            _ => problems.push(KeyProblem::new(
                "conflict_copy_time_zone",
                Reason::WrongType,
            )),
        }
        problems
    }
}

fn at_least(key: &str, min: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: "no ceiling".to_owned(),
        },
    )
}
