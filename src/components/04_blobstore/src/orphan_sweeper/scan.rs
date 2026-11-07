//! The blob volume as the orphan sweeper reads it: one prefix directory at a time, by name only. Every name is
//! classified before it is trusted, and a file is only ever opened or removed through `blob_path` of a blob id
//! that was parsed from a name in its own prefix directory.

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sc_clock::Timestamp;
use uuid::Uuid;

use crate::path::{blob_path, parse_blob_id};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Name {
    /// A blob id in its own prefix directory.
    Blob(Uuid),
    /// `<id>.gc`: a file the GC sweeper has parked. It belongs to that sweeper until it is deleted or restored.
    Parked,
    /// Anything else: not a blob file, so neither reported nor removed.
    Foreign,
}

#[derive(Debug, Default)]
pub(super) struct Listing {
    pub blobs: Vec<Uuid>,
    pub foreign: u64,
}

/// The two folder levels of a blob path as one number: `0xaabb` is `blobs/aa/bb`.
pub(super) fn prefix_dir(root: &Path, prefix: u16) -> PathBuf {
    let [high, low] = prefix.to_be_bytes();
    root.join("blobs")
        .join(format!("{high:02x}"))
        .join(format!("{low:02x}"))
}

/// The smallest and the largest uuid that start with the prefix.
pub(super) fn id_range(prefix: u16) -> (Uuid, Uuid) {
    let [high, low] = prefix.to_be_bytes();
    let mut lo = [0u8; 16];
    let mut hi = [0xffu8; 16];
    (lo[0], lo[1]) = (high, low);
    (hi[0], hi[1]) = (high, low);
    (Uuid::from_bytes(lo), Uuid::from_bytes(hi))
}

pub(super) fn classify(prefix: u16, name: &OsStr, is_file: bool) -> Name {
    let [high, low] = prefix.to_be_bytes();
    let own = format!("{high:02x}{low:02x}");
    let in_prefix = |text: &str| text.starts_with(&own);
    let Some(text) = name.to_str().filter(|_| is_file) else {
        return Name::Foreign;
    };
    match text.strip_suffix(".gc") {
        Some(stem) if in_prefix(stem) && parse_blob_id(stem).is_ok() => Name::Parked,
        Some(_) => Name::Foreign,
        None if in_prefix(text) => parse_blob_id(text).map_or(Name::Foreign, Name::Blob),
        None => Name::Foreign,
    }
}

/// The names in one prefix directory. A directory that does not exist is empty; any other failure, including one
/// while the entries are read, is an error, so the caller never works from a partial listing.
pub(super) async fn list(root: &Path, prefix: u16) -> io::Result<Listing> {
    let mut dir = match tokio::fs::read_dir(prefix_dir(root, prefix)).await {
        Ok(dir) => dir,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Listing::default()),
        Err(error) => return Err(error),
    };
    let mut listing = Listing::default();
    while let Some(entry) = dir.next_entry().await? {
        let is_file = match entry.file_type().await {
            Ok(kind) => kind.is_file(),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        match classify(prefix, &entry.file_name(), is_file) {
            Name::Blob(id) => listing.blobs.push(id),
            Name::Parked => {}
            Name::Foreign => listing.foreign += 1,
        }
    }
    Ok(listing)
}

/// The modification time of the blob's file; `None` when it is gone (the GC sweeper or a writer got there first)
/// or is not a regular file.
pub(super) async fn modified(root: &Path, id: Uuid) -> io::Result<Option<Timestamp>> {
    let meta = match tokio::fs::symlink_metadata(blob_path(root, id)).await {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !meta.is_file() {
        return Ok(None);
    }
    timestamp_of(meta.modified()?).map(Some)
}

fn timestamp_of(time: SystemTime) -> io::Result<Timestamp> {
    let invalid = || io::Error::from(ErrorKind::InvalidData);
    let micros = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_micros()).map_err(|_| invalid())?,
        Err(before) => i64::try_from(before.duration().as_micros())
            .ok()
            .and_then(i64::checked_neg)
            .ok_or_else(invalid)?,
    };
    Ok(Timestamp::from_unix_micros(micros))
}

/// Removes the file; `false` when it was not there any more.
pub(super) async fn remove(root: &Path, id: Uuid) -> io::Result<bool> {
    match tokio::fs::remove_file(blob_path(root, id)).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    const ID: &str = "0a1b2c3d-0000-4000-8000-000000000000";
    const PREFIX: u16 = 0x0a1b;

    fn name(text: &str) -> Name {
        classify(PREFIX, &OsString::from(text), true)
    }

    #[test]
    fn a_blob_id_in_its_own_prefix_directory_is_a_blob() {
        // Act
        let found = name(ID);

        // Assert
        assert_eq!(found, Name::Blob(parse_blob_id(ID).expect("id")));
    }

    #[test]
    fn a_parked_name_is_the_gc_sweepers_and_not_a_blob() {
        // Act
        let found = name(&format!("{ID}.gc"));

        // Assert
        assert_eq!(found, Name::Parked);
    }

    #[test]
    fn names_that_are_not_a_blob_id_in_this_directory_are_foreign() {
        // Arrange
        let upper = ID.to_uppercase();
        let other_prefix = ID.replacen("0a1b", "0a1c", 1);
        let names = [
            "notes.txt",
            ".gc",
            "x.gc",
            &upper,
            &other_prefix,
            &format!("{other_prefix}.gc"),
            &format!("{ID}.gc.gc"),
            &format!("{ID}.tmp"),
            "0a1b2c3d-0000-1000-8000-000000000000",
        ];

        // Act
        let kinds: Vec<Name> = names.iter().map(|text| name(text)).collect();

        // Assert
        assert!(kinds.iter().all(|kind| *kind == Name::Foreign), "{kinds:?}");
    }

    #[test]
    fn a_directory_with_a_blob_name_is_foreign() {
        // Act
        let found = classify(PREFIX, &OsString::from(ID), false);

        // Assert
        assert_eq!(found, Name::Foreign);
    }

    #[test]
    fn the_id_range_covers_exactly_one_prefix() {
        // Act
        let (lo, hi) = id_range(PREFIX);

        // Assert
        assert_eq!(lo.to_string(), "0a1b0000-0000-0000-0000-000000000000");
        assert_eq!(hi.to_string(), "0a1bffff-ffff-ffff-ffff-ffffffffffff");
    }

    #[test]
    fn a_time_before_the_epoch_is_a_negative_timestamp() {
        // Arrange
        let before = UNIX_EPOCH - std::time::Duration::from_secs(1);

        // Act
        let stamp = timestamp_of(before).expect("timestamp");

        // Assert
        assert_eq!(stamp.as_unix_micros(), -1_000_000);
    }
}
