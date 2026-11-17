//! The missing-blob report of `doctor` (E-22/c14b, AC-73): every committed blob whose file is absent from the
//! volume, mapped to the revisions that hold it. Only the composition root sees both 04's inventory and 02's
//! paths, so the join lives here; it is the one missing-blob check `doctor` runs.

use std::sync::Arc;

use contracts::{
    BlobRevision, DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, TreeReader,
    async_trait,
};
use sc_observability::log_error;
use sunnycloud_blobstore::BlobInventory;
use uuid::Uuid;

const REMEDY: &str =
    "restore the blob file from a backup, or remove the row once the blob is known to be gone";

pub struct MissingBlobsReport {
    inventory: Arc<BlobInventory>,
    tree: Arc<dyn TreeReader>,
    batch: u32,
}

impl MissingBlobsReport {
    pub fn new(inventory: Arc<BlobInventory>, tree: Arc<dyn TreeReader>, batch: u32) -> Self {
        MissingBlobsReport {
            inventory,
            tree,
            batch,
        }
    }
}

fn fail(message: String) -> Finding {
    Finding {
        level: FindingLevel::Fail,
        message,
        remedy: REMEDY.to_owned(),
    }
}

/// One `Fail` per revision that holds a missing blob, with its path, account and revision id; a missing blob that
/// no revision holds gets one `Fail` of its own, so every missing blob is reported.
fn findings(missing: &[Uuid], held: &[BlobRevision]) -> Vec<Finding> {
    let mut found: Vec<Finding> = held
        .iter()
        .map(|item| {
            let path = item.path.as_deref().unwrap_or("a deleted file");
            fail(format!(
                "revision {} of account {} ({path}) has no blob file; blob {}",
                item.revision_id, item.account, item.blob_id
            ))
        })
        .collect();
    found.extend(
        missing
            .iter()
            .filter(|blob| held.iter().all(|item| item.blob_id != **blob))
            .map(|blob| {
                fail(format!(
                    "committed blob {blob} has no file on the volume and no revision holds it"
                ))
            }),
    );
    found
}

#[async_trait]
impl DoctorCheck for MissingBlobsReport {
    fn name(&self) -> &'static str {
        "blob_files_missing"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::Restore]
    }

    /// A walk that cannot finish is one `Fail`: the blobs left unchecked are not known to be there.
    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let mut report = Vec::new();
        let mut after = None;
        loop {
            let page = match self.inventory.scan_page(after, self.batch).await {
                Ok(page) => page,
                Err(error) => {
                    log_error!("missing blob scan failed", err = &error);
                    return unfinished(report);
                }
            };
            let held = if page.missing.is_empty() {
                Vec::new()
            } else {
                match self.tree.describe_blob_revisions(&page.missing).await {
                    Ok(held) => held,
                    Err(error) => {
                        log_error!("blob revision lookup failed", err = &error);
                        return unfinished(report);
                    }
                }
            };
            report.extend(findings(&page.missing, &held));
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        if report.is_empty() {
            report.push(Finding {
                level: FindingLevel::Ok,
                message: "every committed blob has its file".to_owned(),
                remedy: String::new(),
            });
        }
        report
    }
}

fn unfinished(mut report: Vec<Finding>) -> Vec<Finding> {
    report.push(Finding {
        level: FindingLevel::Fail,
        message: "the check for missing blob files did not finish".to_owned(),
        remedy: "run doctor again once the database and the volume answer".to_owned(),
    });
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(blob: u128, account: u128, revision: u128, path: Option<&str>) -> BlobRevision {
        BlobRevision {
            blob_id: Uuid::from_u128(blob),
            account: Uuid::from_u128(account),
            revision_id: Uuid::from_u128(revision),
            path: path.map(str::to_owned),
        }
    }

    #[test]
    fn each_revision_of_a_missing_blob_is_one_fail_with_path_account_and_revision() {
        // Arrange
        let missing = [Uuid::from_u128(1), Uuid::from_u128(2)];
        let held = [
            held(1, 10, 100, Some("a/one.txt")),
            held(1, 11, 101, Some("two.txt")),
            held(2, 10, 102, None),
        ];

        // Act
        let found = findings(&missing, &held);

        // Assert
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|item| item.level == FindingLevel::Fail));
        assert!(found[0].message.contains(&Uuid::from_u128(100).to_string()));
        assert!(found[0].message.contains(&Uuid::from_u128(10).to_string()));
        assert!(found[0].message.contains("a/one.txt"));
        assert!(found[2].message.contains("a deleted file"));
    }

    #[test]
    fn a_missing_blob_no_revision_holds_is_still_reported() {
        // Arrange
        let missing = [Uuid::from_u128(1), Uuid::from_u128(9)];
        let held = [held(1, 10, 100, Some("a.txt"))];

        // Act
        let found = findings(&missing, &held);

        // Assert
        assert_eq!(found.len(), 2);
        assert!(found[1].message.contains(&Uuid::from_u128(9).to_string()));
        assert!(found[1].message.contains("no revision holds it"));
    }
}
