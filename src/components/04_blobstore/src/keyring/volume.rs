//! The blob volume as the keyring sees it: the `.keycheck` file and whether any blob file exists.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use contracts::KeyError;

use super::keycheck::KeyCheck;
use crate::error::key_error_from_volume;

const KEY_CHECK_FILE: &str = ".keycheck";
const KEY_CHECK_TEMP: &str = ".keycheck.tmp";
const BLOBS_DIR: &str = "blobs";

/// A volume root that does not exist is not an empty volume: a missing mount must not receive a key check.
pub(crate) async fn existing_root(root: &str) -> Result<PathBuf, KeyError> {
    let path = PathBuf::from(root);
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(key_error_from_volume)?;
    if !meta.is_dir() {
        return Err(key_error_from_volume(io::Error::from(
            ErrorKind::NotADirectory,
        )));
    }
    Ok(path)
}

/// `None` when the file is absent. A file that is not a key check cannot prove the key: `KeyCheckMismatch`.
pub(crate) async fn read_key_check(root: &Path) -> Result<Option<KeyCheck>, KeyError> {
    match tokio::fs::read(root.join(KEY_CHECK_FILE)).await {
        Ok(bytes) => KeyCheck::parse(&bytes)
            .map(Some)
            .ok_or(KeyError::KeyCheckMismatch),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(key_error_from_volume(error)),
    }
}

/// True when any file lies under `blobs/`; folders alone are not data.
pub(crate) async fn has_blob_files(root: &Path) -> Result<bool, KeyError> {
    let mut pending = vec![root.join(BLOBS_DIR)];
    while let Some(dir) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(key_error_from_volume(error)),
        };
        while let Some(entry) = entries.next_entry().await.map_err(key_error_from_volume)? {
            let kind = entry.file_type().await.map_err(key_error_from_volume)?;
            if !kind.is_dir() {
                return Ok(true);
            }
            pending.push(entry.path());
        }
    }
    Ok(false)
}

/// Replaces the file atomically: temp file, `fsync`, rename, `fsync` of the folder.
pub(crate) async fn write_key_check(root: &Path, check: &KeyCheck) -> Result<(), KeyError> {
    let root = root.to_path_buf();
    let bytes = check.encode();
    tokio::task::spawn_blocking(move || replace_file(&root, &bytes))
        .await
        .map_err(|_| key_error_from_volume(io::Error::other("key check write did not finish")))?
        .map_err(key_error_from_volume)
}

fn replace_file(root: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = root.join(KEY_CHECK_TEMP);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, root.join(KEY_CHECK_FILE))?;
    fs::File::open(root)?.sync_all()
}
