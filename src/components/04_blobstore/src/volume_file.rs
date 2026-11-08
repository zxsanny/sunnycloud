//! Small files at the root of the blob volume (`.keycheck`, `.epoch`) are replaced as a whole, never edited.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Writes `bytes` to `<root>/<name>` atomically: temp file `<name>.tmp`, `fsync`, rename, `fsync` of the folder.
/// After a crash at any point the file holds the old or the new content in full; a stale temp file is overwritten
/// by the next call.
pub(crate) fn replace_file(root: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temp = root.join(format!("{name}.tmp"));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, root.join(name))?;
    File::open(root)?.sync_all()
}
