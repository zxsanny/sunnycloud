use std::sync::Arc;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};
use sc_observability::log_error;

use super::{finding, ok};
use crate::inventory::BlobInventory;

const REMEDY: &str =
    "restore the blob file from a backup, or remove the row once the blob is known to be gone";

/// Committed blobs whose file is absent, one `Fail` finding per blob id (a missing blob makes `doctor` exit 2).
/// Walks every committed row in pages of `batch`.
pub struct MissingBlobsCheck {
    inventory: Arc<BlobInventory>,
    batch: u32,
}

impl MissingBlobsCheck {
    pub fn new(inventory: Arc<BlobInventory>, batch: u32) -> Self {
        MissingBlobsCheck { inventory, batch }
    }
}

#[async_trait]
impl DoctorCheck for MissingBlobsCheck {
    fn name(&self) -> &'static str {
        "blob_files_missing"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::Restore]
    }

    /// A walk that cannot finish is one `Fail`: the blobs left unchecked are not known to be there.
    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut after = None;
        loop {
            let page = match self.inventory.scan_page(after, self.batch).await {
                Ok(page) => page,
                Err(error) => {
                    log_error!("missing blob scan failed", err = &error);
                    findings.push(finding(
                        FindingLevel::Fail,
                        "the check for missing blob files did not finish",
                        "run doctor again once the database and the volume answer",
                    ));
                    return findings;
                }
            };
            findings.extend(page.missing.iter().map(|id| {
                finding(
                    FindingLevel::Fail,
                    format!("committed blob {id} has no file on the volume"),
                    REMEDY,
                )
            }));
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        if findings.is_empty() {
            findings.push(ok("every committed blob has its file"));
        }
        findings
    }
}
