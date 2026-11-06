//! Creation of the listening socket: mode 0660 and the shared group, as in 10 section 5.

use std::fs;
use std::os::unix::fs::{PermissionsExt, chown};
use std::os::unix::net::UnixListener;
use std::path::Path;

use crate::error::RunnerError;

const SOCKET_MODE: u32 = 0o660;

/// Binds the socket, replacing the file a previous run left on the shared volume. The group
/// is set before the mode opens the socket to it. The listener is non-blocking.
pub fn bind(path: &Path, gid: Option<u32>) -> Result<UnixListener, RunnerError> {
    remove_stale(path)?;
    let listener = UnixListener::bind(path).map_err(RunnerError::io("bind decoder socket"))?;
    if let Some(gid) = gid {
        chown(path, None, Some(gid)).map_err(RunnerError::io("set decoder socket group"))?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE))
        .map_err(RunnerError::io("set decoder socket mode"))?;
    listener
        .set_nonblocking(true)
        .map_err(RunnerError::io("make decoder socket non-blocking"))?;
    Ok(listener)
}

fn remove_stale(path: &Path) -> Result<(), RunnerError> {
    use std::io::ErrorKind;
    use std::os::unix::fs::FileTypeExt;

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            fs::remove_file(path).map_err(RunnerError::io("remove stale decoder socket"))
        }
        Ok(_) => Err(RunnerError::NotASocket),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RunnerError::Io {
            what: "inspect decoder socket path",
            source,
        }),
    }
}

/// True when `path` is a socket file. Used by `health`; connecting would take the single
/// connection slot away from the supervisor.
pub fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}
