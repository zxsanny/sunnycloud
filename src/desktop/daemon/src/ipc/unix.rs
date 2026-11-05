use std::fs::{self, DirBuilder, Metadata};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ipc_schema::DaemonControl;
use tokio::net::{UnixListener, UnixStream};

use super::error::EndpointRefused;
use super::server::{IpcServer, Listener};

const DIR_NAME: &str = "sunnycloud";
const SOCKET_NAME: &str = "daemon.sock";
const DIR_MODE: u32 = 0o700;
const SOCKET_MODE: u32 = 0o600;

/// Serves `daemon` on `$XDG_RUNTIME_DIR/sunnycloud/daemon.sock` for the running user.
pub async fn start<D>(daemon: Arc<D>) -> Result<IpcServer, EndpointRefused>
where
    D: DaemonControl + Send + Sync + 'static,
{
    let runtime_dir = match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => return Err(log_refusal(EndpointRefused::NoRuntimeDir)),
    };
    start_in(daemon, &runtime_dir, effective_uid()).await
}

/// As [`start`], with the runtime directory and the uid the endpoint must belong to given.
pub async fn start_in<D>(
    daemon: Arc<D>,
    runtime_dir: &Path,
    uid: u32,
) -> Result<IpcServer, EndpointRefused>
where
    D: DaemonControl + Send + Sync + 'static,
{
    match bind(runtime_dir, uid).await {
        Ok(endpoint) => {
            let name = endpoint.path.display().to_string();
            Ok(IpcServer::spawn(daemon, endpoint, name))
        }
        Err(refused) => Err(log_refusal(refused)),
    }
}

fn log_refusal(refused: EndpointRefused) -> EndpointRefused {
    tracing::error!(reason = %refused, "endpoint refused");
    refused
}

pub(super) fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

struct UnixEndpoint {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener for UnixEndpoint {
    type Stream = UnixStream;

    async fn accept(&mut self) -> io::Result<UnixStream> {
        self.listener.accept().await.map(|(stream, _)| stream)
    }

    fn close(self) -> io::Result<()> {
        remove_if_present(&self.path)
    }
}

async fn bind(runtime_dir: &Path, uid: u32) -> Result<UnixEndpoint, EndpointRefused> {
    check_private_dir(runtime_dir, uid)?;
    let dir = runtime_dir.join(DIR_NAME);
    create_private_dir(&dir)?;
    check_private_dir(&dir, uid)?;

    let path = dir.join(SOCKET_NAME);
    clear_leftover_socket(&path, uid).await?;
    let listener =
        UnixListener::bind(&path).map_err(|e| EndpointRefused::io("binding", path.display(), e))?;
    let endpoint = UnixEndpoint { listener, path };
    restrict_socket(&endpoint.path, uid)?;
    Ok(endpoint)
}

fn create_private_dir(dir: &Path) -> Result<(), EndpointRefused> {
    match DirBuilder::new().mode(DIR_MODE).create(dir) {
        // The umask may have narrowed the mode; a directory made here is ours to fix.
        Ok(()) => fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))
            .map_err(|e| EndpointRefused::io("setting the mode of", dir.display(), e)),
        // Whatever is there is judged by `check_private_dir`, never changed.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(EndpointRefused::io("creating", dir.display(), e)),
    }
}

fn stat(path: &Path) -> Result<Metadata, EndpointRefused> {
    fs::symlink_metadata(path).map_err(|e| EndpointRefused::io("inspecting", path.display(), e))
}

fn check_owner_and_mode(
    path: &Path,
    meta: &Metadata,
    uid: u32,
    mode: u32,
) -> Result<(), EndpointRefused> {
    if meta.uid() != uid {
        return Err(EndpointRefused::ForeignOwner {
            path: path.to_path_buf(),
            owner: meta.uid(),
            expected: uid,
        });
    }
    let actual = meta.mode() & 0o7777;
    if actual != mode {
        return Err(EndpointRefused::WrongMode {
            path: path.to_path_buf(),
            mode: actual,
            expected: mode,
        });
    }
    Ok(())
}

/// A real directory (not a symlink) of `uid` with mode exactly 0700.
pub(super) fn check_private_dir(path: &Path, uid: u32) -> Result<(), EndpointRefused> {
    let meta = stat(path)?;
    if !meta.file_type().is_dir() {
        return Err(EndpointRefused::NotADirectory {
            path: path.to_path_buf(),
        });
    }
    check_owner_and_mode(path, &meta, uid, DIR_MODE)
}

/// Applies the leftover rule from the module docs to whatever is at `path`.
pub(super) async fn clear_leftover_socket(path: &Path, uid: u32) -> Result<(), EndpointRefused> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(EndpointRefused::io("inspecting", path.display(), e)),
    };
    if !meta.file_type().is_socket() {
        return Err(EndpointRefused::NotASocket {
            path: path.to_path_buf(),
        });
    }
    check_owner_and_mode(path, &meta, uid, SOCKET_MODE)?;
    match UnixStream::connect(path).await {
        Ok(_) => Err(EndpointRefused::InUse {
            path: path.to_path_buf(),
        }),
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => remove_if_present(path)
            .map_err(|e| EndpointRefused::io("removing the leftover", path.display(), e)),
        Err(e) => Err(EndpointRefused::io("probing", path.display(), e)),
    }
}

/// Binding creates the socket with the umask's mode; the 0700 directory keeps everyone
/// else out until this runs.
fn restrict_socket(path: &Path, uid: u32) -> Result<(), EndpointRefused> {
    let restricted = fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE))
        .map_err(|e| EndpointRefused::io("setting the mode of", path.display(), e))
        .and_then(|()| check_owner_and_mode(path, &stat(path)?, uid, SOCKET_MODE));
    if let Err(refused) = restricted {
        if let Err(error) = remove_if_present(path) {
            tracing::error!(%error, path = %path.display(), "cannot remove the socket that was refused");
        }
        return Err(refused);
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
