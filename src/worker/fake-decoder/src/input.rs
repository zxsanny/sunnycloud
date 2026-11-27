//! What FD finds in a job's `in/source` when a job request arrives: the input file the supervisor fetched and
//! handed to the decoder (09 tests.md IT-09-7 (d) reads these facts from the record).

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use sc_ids::{BLOCK_SIZE, FileHasher};
use serde::Serialize;

/// The size, permission bits and group of the input file, and its file hash in lowercase hex: BLAKE3 per 2 MiB
/// block, rolled up as `revisions.content_hash` is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputFacts {
    pub bytes: u64,
    pub mode: u32,
    pub gid: u32,
    pub hash: String,
}

/// Reads `path` once, a block at a time.
pub(crate) fn facts(path: &Path) -> io::Result<InputFacts> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let mut hasher = FileHasher::new();
    let mut block = vec![0; BLOCK_SIZE];
    loop {
        let filled = fill(&mut file, &mut block)?;
        if filled == 0 {
            break;
        }
        hasher.update(&block[..filled]).map_err(io::Error::other)?;
        hasher.finish_block();
        if filled < BLOCK_SIZE {
            break;
        }
    }
    let (_, hash) = hasher.finish();
    Ok(InputFacts {
        bytes: meta.len(),
        mode: meta.mode() & 0o7777,
        gid: meta.gid(),
        hash: hash.to_string(),
    })
}

/// Reads until `block` is full or the file ends; the count read.
fn fill(file: &mut File, block: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < block.len() {
        match file.read(&mut block[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;

    use sc_ids::{hash_block, rollup};

    use super::*;

    #[test]
    fn facts_of_a_file_past_one_block_hash_each_block_and_roll_them_up() {
        // Arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("source");
        let data: Vec<u8> = (0..BLOCK_SIZE + 5).map(|i| (i % 251) as u8).collect();
        fs::write(&path, &data).expect("write source");
        fs::set_permissions(&path, Permissions::from_mode(0o440)).expect("chmod");
        let expected = rollup(&[
            hash_block(&data[..BLOCK_SIZE]),
            hash_block(&data[BLOCK_SIZE..]),
        ]);

        // Act
        let seen = facts(&path).expect("facts");

        // Assert
        assert_eq!(
            (seen.bytes, seen.mode, seen.hash),
            (data.len() as u64, 0o440, expected.to_string())
        );
    }

    #[test]
    fn facts_of_an_empty_file_are_the_hash_of_no_blocks() {
        // Arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("source");
        fs::write(&path, b"").expect("write source");

        // Act
        let seen = facts(&path).expect("facts");

        // Assert
        assert_eq!((seen.bytes, seen.hash), (0, rollup(&[]).to_string()));
    }

    #[test]
    fn facts_of_a_missing_file_are_an_error() {
        // Arrange
        let dir = tempfile::tempdir().expect("tempdir");

        // Act
        let seen = facts(&dir.path().join("source"));

        // Assert
        assert_eq!(
            seen.map_err(|error| error.kind()),
            Err(io::ErrorKind::NotFound)
        );
    }
}
