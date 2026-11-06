use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use tokio::net::UnixStream;

use super::super::{ClientConfig, ClientError, LocateError};

pub(crate) type Stream = UnixStream;

const DIR_MODE: u32 = 0o700;
const SOCKET_MODE: u32 = 0o600;

pub(crate) fn current_owner() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

pub(crate) fn default_endpoint() -> Result<PathBuf, LocateError> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .ok_or(LocateError::NoRuntimeDir)?;
    Ok(runtime_dir.join("sunnycloud").join("daemon.sock"))
}

/// The rules the daemon applies when it creates the endpoint (`ipc_schema.md`, Transport):
/// the socket is a real socket of the user with mode 0600, in a real directory of the user
/// with mode 0700 under the user's runtime directory. Nothing is connected before the
/// path passes; after the connect the peer's uid is checked too, which closes the window
/// between the path check and the connect.
pub(crate) async fn connect(config: &ClientConfig) -> Result<UnixStream, ClientError> {
    check_path(&config.endpoint, config.owner_uid)?;
    let stream = UnixStream::connect(&config.endpoint)
        .await
        .map_err(connect_error)?;
    let peer = stream
        .peer_cred()
        .map_err(|e| ClientError::Io(format!("reading the peer credentials: {e}")))?;
    if peer.uid() != config.owner_uid {
        return Err(untrusted(format!(
            "the server process runs as uid {}, expected {}",
            peer.uid(),
            config.owner_uid
        )));
    }
    Ok(stream)
}

fn connect_error(error: io::Error) -> ClientError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::DaemonNotRunning,
        _ => ClientError::Io(format!("connecting: {error}")),
    }
}

fn untrusted(reason: String) -> ClientError {
    ClientError::EndpointUntrusted(reason)
}

fn check_path(socket: &Path, uid: u32) -> Result<(), ClientError> {
    let endpoint_dir = socket
        .parent()
        .ok_or_else(|| untrusted("the endpoint path has no directory".to_owned()))?;
    let runtime_dir = endpoint_dir
        .parent()
        .ok_or_else(|| untrusted("the endpoint directory has no parent".to_owned()))?;
    check_dir(runtime_dir, uid)?;
    check_dir(endpoint_dir, uid)?;

    let meta = stat(socket)?;
    if !meta.file_type().is_socket() {
        return Err(untrusted("the endpoint is not a socket".to_owned()));
    }
    check_owner_and_mode(&meta, uid, SOCKET_MODE, "socket")
}

fn check_dir(dir: &Path, uid: u32) -> Result<(), ClientError> {
    let meta = stat(dir)?;
    if !meta.file_type().is_dir() {
        return Err(untrusted(
            "an endpoint directory is not a directory".to_owned(),
        ));
    }
    check_owner_and_mode(&meta, uid, DIR_MODE, "directory")
}

/// Without following symlinks: a symlink at any level is not what the daemon created.
fn stat(path: &Path) -> Result<Metadata, ClientError> {
    fs::symlink_metadata(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ClientError::DaemonNotRunning,
        _ => ClientError::Io(format!("inspecting the endpoint: {e}")),
    })
}

fn check_owner_and_mode(
    meta: &Metadata,
    uid: u32,
    mode: u32,
    what: &str,
) -> Result<(), ClientError> {
    if meta.uid() != uid {
        return Err(untrusted(format!(
            "the {what} belongs to uid {}, expected {uid}",
            meta.uid()
        )));
    }
    let actual = meta.mode() & 0o7777;
    if actual != mode {
        return Err(untrusted(format!(
            "the {what} has mode {actual:04o}, expected {mode:04o}"
        )));
    }
    Ok(())
}
