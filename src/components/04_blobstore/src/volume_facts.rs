//! What the operating system says about the blob volume: can the process write to the root, how much space is
//! left for it, and which filesystem holds it. Read by the health probe and by the volume doctor check.

use std::io;
use std::path::Path;

use rustix::fs::{Access, access, statfs};
use rustix::io::Errno;

use crate::writer::volume_space;

/// `statfs` magic numbers of `<linux/magic.h>`; ext2, ext3 and ext4 share one.
const XFS_MAGIC: u64 = 0x5846_5342;
const EXT_MAGIC: u64 = 0xEF53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filesystem {
    Xfs,
    Ext4,
    Other(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VolumeFacts {
    pub writable: bool,
    /// Bytes an unprivileged writer can still allocate, as the writer's own floor check sees them.
    pub free_bytes: u64,
    /// The volume's total size, for the floor's share of it.
    pub total_bytes: u64,
    pub filesystem: Filesystem,
}

/// A root that does not exist or cannot be queried is an error, not an unwritable volume.
pub(crate) fn inspect(root: &Path) -> io::Result<VolumeFacts> {
    let stat = statfs(root).map_err(io::Error::from)?;
    let writable = match access(root, Access::WRITE_OK) {
        Ok(()) => true,
        Err(Errno::ACCESS | Errno::ROFS | Errno::PERM) => false,
        Err(error) => return Err(io::Error::from(error)),
    };
    let space = volume_space(root)?;
    Ok(VolumeFacts {
        writable,
        free_bytes: space.available,
        total_bytes: space.total,
        filesystem: filesystem(u64::try_from(stat.f_type).unwrap_or(0)),
    })
}

fn filesystem(magic: u64) -> Filesystem {
    match magic {
        XFS_MAGIC => Filesystem::Xfs,
        EXT_MAGIC => Filesystem::Ext4,
        other => Filesystem::Other(other),
    }
}
