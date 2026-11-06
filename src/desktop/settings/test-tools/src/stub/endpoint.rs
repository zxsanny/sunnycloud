//! The local endpoint of the stub, on a path or pipe name unique to this stub instance.
//! On Linux it mirrors the real daemon's layout (`<runtime dir>/sunnycloud/daemon.sock`,
//! directory 0700, socket 0600), so the window finds it when it is started with
//! `XDG_RUNTIME_DIR` set to [`Listener::runtime_dir`] and needs no endpoint override.

use std::sync::atomic::{AtomicU32, Ordering};

static INSTANCE: AtomicU32 = AtomicU32::new(0);

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        INSTANCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(unix)]
pub(crate) use unix::Listener;
#[cfg(windows)]
pub(crate) use windows::Listener;

#[cfg(unix)]
mod unix {
    use std::fs::{self, DirBuilder};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    pub(crate) type Stream = UnixStream;

    pub(crate) struct Listener {
        listener: UnixListener,
        runtime_dir: PathBuf,
        socket: String,
    }

    impl Listener {
        pub(crate) fn bind() -> io::Result<Self> {
            let runtime_dir =
                std::env::temp_dir().join(format!("sc-stub-{}", super::unique_suffix()));
            DirBuilder::new().mode(0o700).create(&runtime_dir)?;
            let dir = runtime_dir.join("sunnycloud");
            DirBuilder::new().mode(0o700).create(&dir)?;
            let path = dir.join("daemon.sock");
            let listener = UnixListener::bind(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            Ok(Self {
                listener,
                socket: path.display().to_string(),
                runtime_dir,
            })
        }

        pub(crate) async fn accept(&mut self) -> io::Result<Stream> {
            self.listener.accept().await.map(|(stream, _)| stream)
        }

        pub(crate) fn endpoint(&self) -> &str {
            &self.socket
        }

        pub(crate) fn runtime_dir(&self) -> &Path {
            &self.runtime_dir
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;

    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    pub(crate) type Stream = NamedPipeServer;

    /// The pipe keeps the default security descriptor (creator, SYSTEM and administrators);
    /// remote clients are rejected and the first instance is exclusive, as the daemon's is.
    pub(crate) struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    impl Listener {
        pub(crate) fn bind() -> io::Result<Self> {
            Self::bind_named(format!(
                r"\\.\pipe\sunnycloud-stub-{}",
                super::unique_suffix()
            ))
        }

        pub(crate) fn bind_named(name: String) -> io::Result<Self> {
            let next = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(&name)?;
            Ok(Self { name, next })
        }

        pub(crate) async fn accept(&mut self) -> io::Result<Stream> {
            self.next.connect().await?;
            let fresh = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.name)?;
            Ok(std::mem::replace(&mut self.next, fresh))
        }

        pub(crate) fn endpoint(&self) -> &str {
            &self.name
        }
    }
}
