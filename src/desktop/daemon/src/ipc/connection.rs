use std::io;
use std::sync::Arc;
use std::time::Duration;

use ipc_schema::{
    DaemonControl, Notification, Response, RpcError, StatusEvent, StatusSink, decode_call,
    dispatch, encode_line,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender, WeakSender};
use tokio::time::timeout;

use super::lines::{CappedLines, MAX_LINE_BYTES};

/// Encoded lines waiting for the writer of one connection. A subscriber that lets this
/// many events pile up is not reading and is disconnected; it reconnects and re-subscribes.
const OUTBOUND_QUEUE: usize = 256;

/// A peer that accepts no bytes for this long is stuck; its connection is closed.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

const READ_CHUNK: usize = 16 * 1024;

/// Delivers `StatusEvent`s of one connection. Holds a weak sender, so a daemon that keeps
/// the sink after the connection closed does not keep the connection's queue alive.
struct ConnectionSink {
    queue: WeakSender<Vec<u8>>,
    overflow: Arc<Notify>,
}

impl StatusSink for ConnectionSink {
    fn send(&self, event: StatusEvent) {
        let Some(queue) = self.queue.upgrade() else {
            return; // the connection is closed; there is nobody to notify
        };
        let line = match Notification::status_event(&event).and_then(|n| encode_line(&n)) {
            Ok(line) => line,
            Err(error) => {
                tracing::error!(%error, "status event cannot be encoded");
                return;
            }
        };
        match queue.try_send(line) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.overflow.notify_one(),
            Err(TrySendError::Closed(_)) => {} // the writer ended; the connection is closing
        }
    }
}

enum ReadEnd {
    PeerClosed,
    LineTooLong,
    WriterGone,
}

/// Serves one client until it disconnects, sends an oversize line, or stops reading.
/// Nothing a client sends ends the process or another connection.
pub(crate) async fn serve_connection<D, S>(daemon: Arc<D>, stream: S)
where
    D: DaemonControl + Send + Sync + 'static,
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let (queue, outbound) = tokio::sync::mpsc::channel(OUTBOUND_QUEUE);
    let overflow = Arc::new(Notify::new());
    let sink: Arc<dyn StatusSink> = Arc::new(ConnectionSink {
        queue: queue.downgrade(),
        overflow: Arc::clone(&overflow),
    });
    let mut writer_task = tokio::spawn(write_loop(writer, outbound));

    let slow_subscriber = tokio::select! {
        end = read_loop(daemon.as_ref(), &sink, &queue, reader) => {
            match end {
                Ok(ReadEnd::PeerClosed | ReadEnd::WriterGone) => {}
                Ok(ReadEnd::LineTooLong) => {
                    tracing::warn!(max = MAX_LINE_BYTES, "request line too long; closing the connection");
                }
                Err(error) => tracing::warn!(%error, "connection read failed"),
            }
            false
        }
        () = overflow.notified() => {
            tracing::warn!(queue = OUTBOUND_QUEUE, "subscriber is not reading; closing the connection");
            true
        }
    };

    drop(queue);
    if slow_subscriber {
        writer_task.abort();
    }
    match (&mut writer_task).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "connection write failed"),
        Err(joined) if joined.is_cancelled() => {}
        Err(joined) => tracing::error!(%joined, "connection writer panicked"),
    }
}

async fn read_loop<D, R>(
    daemon: &D,
    sink: &Arc<dyn StatusSink>,
    queue: &Sender<Vec<u8>>,
    mut reader: R,
) -> io::Result<ReadEnd>
where
    D: DaemonControl + Sync,
    R: AsyncRead + Unpin,
{
    let mut lines = CappedLines::new(MAX_LINE_BYTES);
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(ReadEnd::PeerClosed);
        }
        if lines.push(&chunk[..read]).is_err() {
            let error = RpcError::invalid_request(format!(
                "request line longer than {MAX_LINE_BYTES} bytes"
            ));
            reply(queue, &Response::failure(None, error)).await;
            return Ok(ReadEnd::LineTooLong);
        }
        while let Some(line) = lines.next_line() {
            let response = match decode_call(&line) {
                Ok(call) => dispatch(daemon, sink, call.id, call.call).await,
                Err(response) => response,
            };
            if !reply(queue, &response).await {
                return Ok(ReadEnd::WriterGone);
            }
        }
    }
}

/// `false` when the response cannot be delivered because the writer ended.
async fn reply(queue: &Sender<Vec<u8>>, response: &Response) -> bool {
    match encode_line(response) {
        Ok(line) => queue.send(line).await.is_ok(),
        Err(error) => {
            tracing::error!(%error, "response cannot be encoded");
            false
        }
    }
}

async fn write_loop<W>(mut writer: W, mut outbound: Receiver<Vec<u8>>) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(line) = outbound.recv().await {
        timeout(WRITE_TIMEOUT, writer.write_all(&line))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "peer stopped reading"))??;
    }
    writer.flush().await
}
