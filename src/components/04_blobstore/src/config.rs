use std::collections::BTreeSet;
use std::time::Duration;

use sc_config::{KeyProblem, Reason, Secret, Settings};

const ORPHAN_MODES: [&str; 3] = ["off", "report", "delete"];
/// The id of the key in `master_key_file`; the ring adds the others.
pub(crate) const FILE_KEY_ID: i16 = 1;

/// The blobstore section of the key table, typed. Defaults and per-key ranges (`retention` 0 to 90 days in
/// production) come from `sc_config`.
#[derive(Debug, Clone)]
pub struct BlobstoreConfig {
    pub blob_root: Option<String>,
    pub retention: Duration,
    pub orphan_sweep_age: Duration,
    /// `off`, `report` or `delete`; any other text is refused by [`BlobstoreConfig::validate`].
    pub orphan_mode: String,
    pub orphan_sweep_interval: Duration,
    /// The free-space floor is the smaller of this and `blob_free_reserve_percent` of the volume (owner decision
    /// 2026-11-13).
    pub blob_free_reserve: u64,
    pub blob_free_reserve_percent: u64,
    pub gc_interval: Duration,
    pub gc_batch: u64,
    pub gc_claim_lease: Duration,
    pub gc_cycle_budget: Duration,
    pub scrub_interval: Duration,
    pub scrub_cycle_budget: Duration,
    pub scrub_max_rate: u64,
    pub scrub_pause_active_streams: u64,
    pub blob_sync_coalesce_delay: Duration,
    pub blob_preallocate: bool,
    pub blob_read_ahead_chunks: u64,
    pub blob_io_threads: Option<u64>,
    pub master_key_file: Secret<String>,
    /// `id=path,...`; checked by [`BlobstoreConfig::validate`], read by the keyring.
    pub master_key_ring: Option<String>,
    pub master_key_current: Option<u64>,
}

impl BlobstoreConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        BlobstoreConfig {
            blob_root: settings.blob_root.clone(),
            retention: settings.retention.get(),
            orphan_sweep_age: settings.orphan_sweep_age.get(),
            orphan_mode: settings.orphan_mode.clone(),
            orphan_sweep_interval: settings.orphan_sweep_interval.get(),
            blob_free_reserve: settings.blob_free_reserve.bytes(),
            blob_free_reserve_percent: settings.blob_free_reserve_percent,
            gc_interval: settings.gc_interval.get(),
            gc_batch: settings.gc_batch,
            gc_claim_lease: settings.gc_claim_lease.get(),
            gc_cycle_budget: settings.gc_cycle_budget.get(),
            scrub_interval: settings.scrub_interval.get(),
            scrub_cycle_budget: settings.scrub_cycle_budget.get(),
            scrub_max_rate: settings.scrub_max_rate.bytes(),
            scrub_pause_active_streams: settings.scrub_pause_active_streams,
            blob_sync_coalesce_delay: settings.blob_sync_coalesce_delay.get(),
            blob_preallocate: settings.blob_preallocate,
            blob_read_ahead_chunks: settings.blob_read_ahead_chunks,
            blob_io_threads: settings.blob_io_threads,
            master_key_file: settings.master_key_file.clone(),
            master_key_ring: settings.master_key_ring.clone(),
            master_key_current: settings.master_key_current,
        }
    }

    /// Start validation: every problem, each naming its key. A zero interval or budget would spin or do no
    /// work, a zero count or rate would stall a loop, and a current key id must be one the ring can load.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        if self.blob_root.as_deref().is_none_or(str::is_empty) {
            problems.push(KeyProblem::new("blob_root", Reason::MissingRequired));
        }
        if !ORPHAN_MODES.contains(&self.orphan_mode.as_str()) {
            problems.push(KeyProblem::new("orphan_mode", Reason::WrongType));
        }
        for (key, value) in [
            ("orphan_sweep_interval", self.orphan_sweep_interval),
            ("gc_interval", self.gc_interval),
            ("gc_claim_lease", self.gc_claim_lease),
            ("gc_cycle_budget", self.gc_cycle_budget),
            ("scrub_interval", self.scrub_interval),
            ("scrub_cycle_budget", self.scrub_cycle_budget),
        ] {
            if value.is_zero() {
                problems.push(at_least_one(key, "1ms"));
            }
        }
        for (key, value) in [
            ("gc_batch", Some(self.gc_batch)),
            ("scrub_max_rate", Some(self.scrub_max_rate)),
            ("blob_read_ahead_chunks", Some(self.blob_read_ahead_chunks)),
            ("blob_io_threads", self.blob_io_threads),
        ] {
            if value == Some(0) {
                problems.push(at_least_one(key, "1"));
            }
        }
        self.check_keys(&mut problems);
        problems
    }

    fn check_keys(&self, problems: &mut Vec<KeyProblem>) {
        let mut known = BTreeSet::from([FILE_KEY_ID]);
        if let Some(ring) = &self.master_key_ring {
            match parse_ring(ring) {
                Some(entries) => {
                    let file = self.master_key_file.expose().as_str();
                    if entries
                        .iter()
                        .any(|(id, path)| *id == FILE_KEY_ID && *path != file)
                    {
                        problems.push(KeyProblem::new("master_key_ring", Reason::WrongType));
                    }
                    known.extend(entries.iter().map(|(id, _)| *id));
                }
                None => problems.push(KeyProblem::new("master_key_ring", Reason::WrongType)),
            }
        }
        if let Some(current) = self.master_key_current {
            let held = i16::try_from(current).is_ok_and(|id| known.contains(&id));
            if !held {
                problems.push(KeyProblem::new("master_key_current", Reason::WrongType));
            }
        }
    }
}

fn at_least_one(key: &str, min: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: "no ceiling".to_owned(),
        },
    )
}

/// The `(id, path)` entries of a ring written `id=path,...`; `None` when an entry is not `<positive id>=<path>`
/// or an id repeats.
pub(crate) fn parse_ring(ring: &str) -> Option<Vec<(i16, &str)>> {
    let mut entries: Vec<(i16, &str)> = Vec::new();
    for entry in ring.split(',') {
        let (id, path) = entry.split_once('=')?;
        let id = id.trim().parse::<i16>().ok().filter(|id| *id > 0)?;
        let path = path.trim();
        if path.is_empty() || entries.iter().any(|(seen, _)| *seen == id) {
            return None;
        }
        entries.push((id, path));
    }
    Some(entries)
}
