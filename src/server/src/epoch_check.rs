//! The epoch check of `doctor` (AZ-3779 AC-6, AZ-3666 AC-1): the database epoch against the `.epoch` marker,
//! where an absent marker is a problem only on a volume that holds blobs. 05 owns the rule (`epoch_findings`) and
//! the marker; whether the volume holds blobs comes from 04's inventory, which 05 may not read, so the two meet
//! here, as the missing-blob report does (owner decision 2026-11-18). "Holds blobs" means the database records at
//! least one committed blob (04's rows), not that files exist on the volume: a restored database on an empty
//! volume is the missing-blob report's finding, and an absent marker beside committed rows is this check's.

use std::sync::Arc;

use contracts::{
    DoctorCheck, DoctorContext, DoctorTag, EpochAdmin, Finding, FindingLevel, Marker, async_trait,
};
use sc_observability::log_error;
use sunnycloud_blobstore::BlobInventory;
use sunnycloud_sync_feed::epoch_findings;

const REMEDY: &str = "check the database connection and the blob volume, then run doctor again";

pub struct EpochCheck {
    epoch: Arc<dyn EpochAdmin>,
    inventory: Arc<BlobInventory>,
}

impl EpochCheck {
    pub fn new(epoch: Arc<dyn EpochAdmin>, inventory: Arc<BlobInventory>) -> Self {
        EpochCheck { epoch, inventory }
    }

    /// Whether at least one committed blob is recorded: a page of one row is full, so its `next` is set exactly
    /// when a committed row exists.
    async fn holds_blobs(&self) -> Result<bool, Finding> {
        match self.inventory.scan_page(None, 1).await {
            Ok(page) => Ok(page.next.is_some()),
            Err(error) => {
                log_error!("epoch check: blob inventory failed", err = &error);
                Err(unknown("the blob inventory cannot be read"))
            }
        }
    }
}

fn unknown(message: &str) -> Finding {
    Finding {
        level: FindingLevel::Fail,
        message: format!("the epoch cannot be checked: {message}"),
        remedy: REMEDY.to_owned(),
    }
}

#[async_trait]
impl DoctorCheck for EpochCheck {
    fn name(&self) -> &'static str {
        "epoch"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::Restore]
    }

    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let status = match self.epoch.marker_status().await {
            Ok(status) => status,
            Err(error) => {
                log_error!("epoch check: marker status failed", err = &error);
                return vec![unknown(
                    "the database epoch or the .epoch marker cannot be read",
                )];
            }
        };
        // `epoch_findings` reads the volume only for an absent marker; a present marker needs no inventory read.
        let holds_blobs = if matches!(status.marker, Marker::Absent) {
            match self.holds_blobs().await {
                Ok(holds_blobs) => holds_blobs,
                Err(finding) => return vec![finding],
            }
        } else {
            false
        };
        epoch_findings(status, holds_blobs)
    }
}
