use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::format_error::FormatError;

/// The server-generated blob id in its only accepted spelling: lowercase, hyphenated, version 4.
pub fn parse_blob_id(text: &str) -> Result<Uuid, FormatError> {
    let id = Uuid::try_parse(text).map_err(|_| FormatError::InvalidBlobId)?;
    let mut buffer = Uuid::encode_buffer();
    let canonical = id.hyphenated().encode_lower(&mut buffer) == text;
    if id.get_version_num() == 4 && canonical {
        Ok(id)
    } else {
        Err(FormatError::InvalidBlobId)
    }
}

/// `<root>/blobs/<id[0:2]>/<id[2:4]>/<id>`. Built from a [`Uuid`] only, so no text of a request can add a
/// separator or a `..` segment.
pub fn blob_path(root: &Path, id: Uuid) -> PathBuf {
    let mut buffer = Uuid::encode_buffer();
    let text: &str = id.hyphenated().encode_lower(&mut buffer);
    root.join("blobs")
        .join(&text[0..2])
        .join(&text[2..4])
        .join(text)
}
