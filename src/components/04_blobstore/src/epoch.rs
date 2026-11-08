//! The `.epoch` marker: the installation epoch as decimal text in `<blob_root>/.epoch`.
//!
//! This module is the only access to the file. The value is never decided here: 05 writes it when it picks the
//! installation epoch and after each bump, and 12 compares it with the database at `serve` start. The file is
//! `<epoch>\n` and is replaced atomically, so a crash leaves the old or the new value in full; the terminating
//! newline is what lets `read` tell a complete file from a cut one.

use std::io::{self, ErrorKind};
use std::path::PathBuf;

use contracts::{BlobError, EpochMarker, EpochMarkerError, async_trait};

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io, io_fault};
use crate::volume_file::replace_file;

const EPOCH_FILE: &str = ".epoch";

pub struct FileEpochMarker {
    root: PathBuf,
}

impl FileEpochMarker {
    /// `config.blob_root` is required.
    pub fn new(config: &BlobstoreConfig) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), BlobIds::default())
            })?;
        Ok(FileEpochMarker { root: root.into() })
    }
}

fn encode(epoch: i64) -> Vec<u8> {
    format!("{epoch}\n").into_bytes()
}

/// The canonical decimal text of an `i64` and a newline; anything else (empty, cut, padded, signed with `+`, not
/// a number) is not a value.
fn parse(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(bytes.strip_suffix(b"\n")?).ok()?;
    let epoch: i64 = text.parse().ok()?;
    (epoch.to_string() == text).then_some(epoch)
}

fn marker_error(error: &io::Error) -> EpochMarkerError {
    EpochMarkerError::Io(io_fault(error, BlobIds::default()))
}

#[async_trait]
impl EpochMarker for FileEpochMarker {
    async fn write(&self, epoch: i64) -> Result<(), EpochMarkerError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || replace_file(&root, EPOCH_FILE, &encode(epoch)))
            .await
            .map_err(|_| marker_error(&io::Error::other("epoch marker write did not finish")))?
            .map_err(|error| marker_error(&error))
    }

    /// `None` when the file is absent on a volume that is there; a root that does not exist is an error, so an
    /// unmounted volume is not read as a fresh one. A file that is not a complete marker is an error, never a
    /// value.
    async fn read(&self) -> Result<Option<i64>, EpochMarkerError> {
        match tokio::fs::read(self.root.join(EPOCH_FILE)).await {
            Ok(bytes) => parse(&bytes)
                .map(Some)
                .ok_or_else(|| marker_error(&io::Error::from(ErrorKind::InvalidData))),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                tokio::fs::metadata(&self.root)
                    .await
                    .map_err(|error| marker_error(&error))?;
                Ok(None)
            }
            Err(error) => Err(marker_error(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{encode, parse};

    #[test]
    fn a_complete_marker_parses_to_its_value() {
        // Arrange
        let cases = [(0, "0\n"), (7, "7\n"), (i64::MAX, "9223372036854775807\n")];

        // Act
        let parsed: Vec<_> = cases
            .iter()
            .map(|(epoch, text)| (encode(*epoch), parse(text.as_bytes())))
            .collect();

        // Assert
        for ((epoch, text), (encoded, value)) in cases.iter().zip(parsed) {
            assert_eq!(encoded, text.as_bytes());
            assert_eq!(value, Some(*epoch));
        }
    }

    #[test]
    fn a_cut_or_malformed_marker_is_never_a_value() {
        // Arrange
        let cases: [&[u8]; 9] = [
            b"",
            b"\n",
            b"12",
            b"12x\n",
            b"+7\n",
            b"007\n",
            b" 7\n",
            b"7\n\n",
            b"9223372036854775808\n",
        ];

        // Act
        let values: Vec<_> = cases.iter().map(|bytes| parse(bytes)).collect();

        // Assert
        assert!(values.iter().all(Option::is_none), "{values:?}");
    }
}
