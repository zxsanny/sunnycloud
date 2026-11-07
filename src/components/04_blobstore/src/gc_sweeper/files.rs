//! The blob file during removal. A due file is first renamed to `<path>.gc` (parked): a reinstate that stats the
//! file afterwards sees it absent, and the file is deleted only after its rows are gone, so a blob that was
//! reinstated in the meantime can be moved back and is never lost. No database transaction is open here.

use std::ffi::OsString;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::path::blob_path;

pub(super) struct Files {
    root: PathBuf,
}

pub(super) enum Park {
    Moved,
    /// A previous cycle parked it and did not finish.
    Earlier,
    Missing,
}

fn parked_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path);
    name.push(".gc");
    PathBuf::from(name)
}

impl Files {
    pub(super) fn new(root: PathBuf) -> Self {
        Files { root }
    }

    pub(super) async fn park(&self, id: Uuid) -> io::Result<Park> {
        let path = blob_path(&self.root, id);
        let parked = parked_path(&path);
        match tokio::fs::rename(&path, &parked).await {
            Ok(()) => Ok(Park::Moved),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                match tokio::fs::metadata(&parked).await {
                    Ok(_) => Ok(Park::Earlier),
                    Err(error) if error.kind() == ErrorKind::NotFound => Ok(Park::Missing),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Moves a parked file back for a blob that is still alive.
    pub(super) async fn unpark(&self, id: Uuid) -> io::Result<()> {
        let path = blob_path(&self.root, id);
        tokio::fs::rename(parked_path(&path), &path).await
    }

    /// Deletes the parked file of a blob whose rows are gone. A file that is not there is the normal case of a
    /// blob that was already missing.
    pub(super) async fn discard(&self, id: Uuid) -> io::Result<()> {
        match tokio::fs::remove_file(parked_path(&blob_path(&self.root, id))).await {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
