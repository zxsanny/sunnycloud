//! The listening socket, set up the way the real decoder sets up its own (10 section 5): a stale socket that a
//! previous run left on the shared volume is replaced, any other file at the path is an error, and the group is set
//! before mode 0660 opens the socket to it. FD does this itself instead of linking the decoder: component 10 has no
//! public API, and 09 reaches it only over the socket.

use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::{FileTypeExt, PermissionsExt, chown};
use std::os::unix::net::UnixListener;
use std::path::Path;

const SOCKET_MODE: u32 = 0o660;

/// A non-blocking listener on `path`.
pub(crate) fn bind(path: &Path, gid: Option<u32>) -> io::Result<UnixListener> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path)?,
        Ok(_) => {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("{} exists and is not a socket", path.display()),
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    if let Some(gid) = gid {
        chown(path, None, Some(gid))?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_replaces_a_stale_socket_and_opens_it_to_the_group_only() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("decoder.sock");
        drop(UnixListener::bind(&path).expect("a stale socket"));

        // Act
        let listener = bind(&path, None);

        // Assert
        assert!(listener.is_ok(), "{listener:?}");
        let mode = fs::metadata(&path).expect("socket").permissions().mode() & 0o777;
        assert_eq!(mode, SOCKET_MODE);
    }

    #[test]
    fn bind_refuses_a_path_that_holds_another_kind_of_file() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("decoder.sock");
        fs::write(&path, b"not a socket").expect("a regular file");

        // Act
        let refused = bind(&path, None);

        // Assert
        assert_eq!(
            refused.map(drop).map_err(|error| error.kind()),
            Err(ErrorKind::AlreadyExists)
        );
        assert_eq!(fs::read(&path).expect("left alone"), b"not a socket");
    }
}
