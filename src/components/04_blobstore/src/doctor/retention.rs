use std::time::Duration;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};

use super::{finding, ok};
use crate::config::BlobstoreConfig;

/// Retention against the safety net it is meant to give: not zero, and not shorter than the operator's backup
/// interval (`--backup-interval`), because a backup older than the retention can reference blobs the collector
/// has already removed.
pub struct RetentionCheck {
    retention: Duration,
}

impl RetentionCheck {
    pub fn new(config: &BlobstoreConfig) -> Self {
        RetentionCheck {
            retention: config.retention,
        }
    }
}

#[async_trait]
impl DoctorCheck for RetentionCheck {
    fn name(&self) -> &'static str {
        "blob_retention"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[]
    }

    /// Both findings are `Warn`. Without `--backup-interval` only the zero retention is judged.
    async fn run(&self, ctx: &DoctorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        if self.retention.is_zero() {
            findings.push(finding(
                FindingLevel::Warn,
                "retention is 0: replaced and deleted blobs are removed at once, there is no safety net",
                "set SUNNYCLOUD_RETENTION above the backup interval, unless volume snapshots cover it",
            ));
        }
        if let Some(interval) = ctx.backup_interval
            && self.retention < interval
        {
            findings.push(finding(
                FindingLevel::Warn,
                format!(
                    "retention {}s is shorter than the backup interval {}s",
                    self.retention.as_secs(),
                    interval.as_secs()
                ),
                "raise SUNNYCLOUD_RETENTION above the backup interval, or back up more often",
            ));
        }
        if findings.is_empty() {
            findings.push(ok(
                "retention is set and not shorter than the backup interval",
            ));
        }
        findings
    }
}
