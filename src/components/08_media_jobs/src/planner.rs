//! Job planning at commit (E-18/c06): which jobs a published entry needs and at which priority. Pure: no I/O, no
//! clock; the effect supplies the queued-jobs count it probed.

use contracts::PublishedEntry;
use media_shared::policy::{CURRENT_PARAMS_VERSION, JobSpec, supported_kinds_for};
use uuid::Uuid;

use crate::config::MediaConfig;
use crate::job_queue::Priority;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPlanner {
    params_version: u32,
    bulk_queue_threshold: u64,
}

impl JobPlanner {
    pub const fn new(params_version: u32, bulk_queue_threshold: u64) -> Self {
        JobPlanner {
            params_version,
            bulk_queue_threshold,
        }
    }

    /// Plans with the current `params_version` and the configured threshold.
    pub fn from_config(config: &MediaConfig) -> Self {
        JobPlanner::new(CURRENT_PARAMS_VERSION, config.bulk_queue_threshold)
    }

    pub const fn params_version(&self) -> u32 {
        self.params_version
    }

    /// One job per kind of `supported_kinds_for`; none for a vault file, an empty file or an entry without a
    /// media type.
    pub fn plan(&self, account_id: Uuid, entry: &PublishedEntry) -> Vec<JobSpec> {
        let media_type = match entry.media_type.as_deref() {
            Some(media_type) if !entry.vault && entry.size > 0 => media_type,
            _ => return Vec::new(),
        };
        supported_kinds_for(media_type)
            .into_iter()
            .map(|kind| JobSpec {
                account_id,
                revision_id: entry.revision_id,
                kind,
                content_hash: entry.content_hash,
                params_version: self.params_version,
            })
            .collect()
    }

    /// The `LIMIT` of the queued-jobs count: one more than the threshold is enough to tell "more than".
    pub const fn count_limit(&self) -> u64 {
        self.bulk_queue_threshold.saturating_add(1)
    }

    /// Bulk when the entry carries the import hint or the account already has more than the threshold queued.
    pub const fn priority(&self, import_hint: bool, queued: u64) -> Priority {
        if import_hint || queued > self.bulk_queue_threshold {
            Priority::Bulk
        } else {
            Priority::Interactive
        }
    }
}

#[cfg(test)]
mod tests;
