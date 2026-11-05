use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use ipc_schema::DaemonControl;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};

use super::connection::serve_connection;

/// Pause after a failed `accept` (for example out of file descriptors), so a persistent
/// failure does not spin.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// The OS-specific half of the server: owns the endpoint and hands out connections.
pub(crate) trait Listener: Send + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Stream>> + Send;

    /// Removes the endpoint from the file system, where it has an entry there.
    fn close(self) -> io::Result<()>;
}

/// The running IPC server: an accept loop and one task per connection.
#[derive(Debug)]
pub struct IpcServer {
    endpoint: String,
    stop: oneshot::Sender<()>,
    task: JoinHandle<io::Result<()>>,
}

impl IpcServer {
    pub(crate) fn spawn<D, L>(daemon: Arc<D>, listener: L, endpoint: String) -> Self
    where
        D: DaemonControl + Send + Sync + 'static,
        L: Listener,
    {
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(accept_loop(daemon, listener, stopped));
        Self {
            endpoint,
            stop,
            task,
        }
    }

    /// The socket path or pipe name clients connect to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Stops accepting, closes every connection and removes the endpoint.
    pub async fn shutdown(self) -> io::Result<()> {
        if self.stop.send(()).is_err() {
            // The accept loop already ended; its result is read below.
        }
        self.task.await.map_err(io::Error::other)?
    }
}

async fn accept_loop<D, L>(
    daemon: Arc<D>,
    mut listener: L,
    mut stopped: oneshot::Receiver<()>,
) -> io::Result<()>
where
    D: DaemonControl + Send + Sync + 'static,
    L: Listener,
{
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    connections.spawn(serve_connection(Arc::clone(&daemon), stream));
                }
                Err(error) => {
                    tracing::error!(%error, "accepting an ipc connection failed");
                    tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                }
            },
            Some(joined) = connections.join_next() => {
                if let Err(joined) = joined
                    && joined.is_panic()
                {
                    tracing::error!(%joined, "an ipc connection task panicked");
                }
            }
        }
    }
    connections.shutdown().await;
    listener.close()
}
