//! The blob volume as the writer sees it: free space against the floor, the folders of a blob, its file with
//! header and reserved space, and the directory sync that makes the new name durable.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::Path;

use contracts::BlobError;
use rustix::fs::{FallocateFlags, fallocate, statvfs};
use sc_observability::log_error;
use uuid::Uuid;

use crate::header::Header;
use crate::path::blob_path;

/// The volume that holds `root`: bytes a non-privileged writer can still allocate, and its total size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VolumeSpace {
    pub available: u64,
    pub total: u64,
}

pub(crate) fn volume_space(root: &Path) -> io::Result<VolumeSpace> {
    let stat = statvfs(root).map_err(io::Error::from)?;
    Ok(VolumeSpace {
        available: stat.f_bavail.saturating_mul(stat.f_frsize),
        total: stat.f_blocks.saturating_mul(stat.f_frsize),
    })
}

/// The free-space floor: the smaller of `cap` (`blob_free_reserve`) and `percent` of the volume's `total` size
/// (`blob_free_reserve_percent`; owner decision 2026-11-13), so a small volume is not refused at its first file.
pub(crate) fn free_floor(cap: u64, percent: u64, total: u64) -> u64 {
    let share = u128::from(total) * u128::from(percent.min(100)) / 100;
    cap.min(u64::try_from(share).unwrap_or(u64::MAX))
}

/// With a floor configured, a file that fits but would leave less than `reserve` free is `FreeSpaceFloor`; a
/// file that does not fit at all is `NoSpace`, as is the operating system's own refusal later.
pub(super) fn admit(available: u64, needed: u64, reserve: u64) -> Result<(), BlobError> {
    if available < needed {
        Err(BlobError::NoSpace)
    } else if available - needed < reserve {
        Err(BlobError::FreeSpaceFloor)
    } else {
        Ok(())
    }
}

/// Creates the file, writes the header, reserves `len` bytes when asked, and syncs the directory entry once.
/// A failure after the file exists removes it; a file that already existed is never touched.
pub(super) fn create_blob_file(
    root: &Path,
    blob_id: Uuid,
    header: &Header,
    len: u64,
    preallocate: bool,
) -> io::Result<File> {
    let path = blob_path(root, blob_id);
    let folder = path.parent().ok_or(ErrorKind::InvalidInput)?;
    create_folders(root, folder)?;
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    match prepare(&file, folder, header, len, preallocate) {
        Ok(()) => Ok(file),
        Err(error) => {
            if let Err(removal) = fs::remove_file(&path) {
                log_error!("partial blob file not removed", err = &removal);
            }
            Err(error)
        }
    }
}

fn prepare(
    file: &File,
    folder: &Path,
    header: &Header,
    len: u64,
    preallocate: bool,
) -> io::Result<()> {
    file.write_all_at(&header.encode(), 0)?;
    if preallocate {
        fallocate(file, FallocateFlags::empty(), 0, len).map_err(io::Error::from)?;
    }
    sync_folder(folder)
}

/// `blobs/<xx>/<yy>` below `root`; a folder this call creates is made durable in its parent, so the file's own
/// directory sync covers a chain that survives a crash.
fn create_folders(root: &Path, folder: &Path) -> io::Result<()> {
    let relative = folder
        .strip_prefix(root)
        .map_err(|_| ErrorKind::InvalidInput)?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let parent = current.clone();
        current.push(part);
        match fs::create_dir(&current) {
            Ok(()) => sync_folder(&parent)?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn the_floor_is_the_smaller_of_the_cap_and_the_share_of_the_volume() {
        // Arrange
        let cases = [
            (10 * GIB, 5, 100 * GIB),
            (10 * GIB, 5, 1000 * GIB),
            (10 * GIB, 0, 1000 * GIB),
            (10 * GIB, 100, 4 * GIB),
            (u64::MAX, 100, u64::MAX),
        ];

        // Act
        let floors = cases.map(|(cap, percent, total)| free_floor(cap, percent, total));

        // Assert
        assert_eq!(floors, [5 * GIB, 10 * GIB, 0, 4 * GIB, u64::MAX]);
    }
}
