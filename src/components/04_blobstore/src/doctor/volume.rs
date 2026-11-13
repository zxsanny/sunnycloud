use std::path::PathBuf;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};
use sc_observability::log_error;

use super::{finding, ok};
use crate::config::BlobstoreConfig;
use crate::volume_facts::{self, Filesystem};
use crate::writer::free_floor;

/// The blob volume: reachable and writable, on XFS or ext4, with free space at or above the floor (the smaller of
/// `blob_free_reserve` and its share of the volume).
/// Needs no database, so it runs in a pre-flight.
pub struct VolumeCheck {
    root: Option<PathBuf>,
    reserve: u64,
    reserve_percent: u64,
}

impl VolumeCheck {
    pub fn new(config: &BlobstoreConfig) -> Self {
        VolumeCheck {
            root: config
                .blob_root
                .as_deref()
                .filter(|root| !root.is_empty())
                .map(PathBuf::from),
            reserve: config.blob_free_reserve,
            reserve_percent: config.blob_free_reserve_percent,
        }
    }
}

#[async_trait]
impl DoctorCheck for VolumeCheck {
    fn name(&self) -> &'static str {
        "blob_volume"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::PreFlight]
    }

    /// A volume that is missing or not writable, and free space under the floor, are `Fail`; a filesystem that is
    /// neither XFS nor ext4 is `Warn`.
    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let Some(root) = self.root.clone() else {
            return vec![finding(
                FindingLevel::Fail,
                "blob_root is not set",
                "set SUNNYCLOUD_BLOB_ROOT to the mounted blob volume",
            )];
        };
        let facts = match tokio::task::spawn_blocking(move || volume_facts::inspect(&root)).await {
            Ok(Ok(facts)) => facts,
            Ok(Err(error)) => {
                log_error!("blob volume not inspected", err = &error);
                return vec![finding(
                    FindingLevel::Fail,
                    format!("blob volume cannot be read ({})", error.kind()),
                    "mount the blob volume in the api container at blob_root",
                )];
            }
            Err(error) => {
                log_error!("blob volume check did not finish", err = &error);
                return vec![finding(
                    FindingLevel::Fail,
                    "blob volume check did not finish",
                    "run doctor again",
                )];
            }
        };
        let writable = if facts.writable {
            ok("blob volume is writable")
        } else {
            finding(
                FindingLevel::Fail,
                "blob volume is not writable",
                "mount the blob volume read-write and give the api user write access",
            )
        };
        let filesystem = match facts.filesystem {
            Filesystem::Xfs => ok("blob volume is on XFS"),
            Filesystem::Ext4 => ok("blob volume is on ext4"),
            Filesystem::Other(magic) => finding(
                FindingLevel::Warn,
                format!("blob volume filesystem (type 0x{magic:x}) is neither XFS nor ext4"),
                "move the blob volume to XFS (the documented default) or ext4",
            ),
        };
        let floor = free_floor(self.reserve, self.reserve_percent, facts.total_bytes);
        let space = if facts.free_bytes < floor {
            finding(
                FindingLevel::Fail,
                format!(
                    "free space {} bytes is under the floor of {} bytes",
                    facts.free_bytes, floor
                ),
                "free space on the blob volume or enlarge it",
            )
        } else {
            ok(format!(
                "free space {} bytes is at or above the floor of {} bytes",
                facts.free_bytes, floor
            ))
        };
        vec![writable, filesystem, space]
    }
}
