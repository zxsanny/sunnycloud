//! A socket that ends a connection whose peer stops reading (`api_v1_surface.md`, time limits: download
//! write-idle). A write or flush that makes no progress for `limit` fails with `TimedOut`, which closes the
//! connection; any progress restarts the clock. It guards every response write on the connection, so a
//! download to a stalled client cannot hold a task and a descriptor open.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Sleep, sleep};

pub(crate) struct WriteIdle<T> {
    inner: T,
    limit: Duration,
    stalled: Option<Pin<Box<Sleep>>>,
}

impl<T> WriteIdle<T> {
    pub(crate) fn new(inner: T, limit: Duration) -> Self {
        Self {
            inner,
            limit,
            stalled: None,
        }
    }

    /// Runs one write-side operation of the socket; a pending result arms the timer, a ready one clears it.
    fn guard<R>(
        &mut self,
        cx: &mut Context<'_>,
        op: impl FnOnce(&mut T, &mut Context<'_>) -> Poll<io::Result<R>>,
    ) -> Poll<io::Result<R>> {
        if let Poll::Ready(result) = op(&mut self.inner, cx) {
            self.stalled = None;
            return Poll::Ready(result);
        }
        let limit = self.limit;
        let timer = self.stalled.get_or_insert_with(|| Box::pin(sleep(limit)));
        if timer.as_mut().poll(cx).is_ready() {
            self.stalled = None;
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "peer stopped reading the response",
            )));
        }
        Poll::Pending
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for WriteIdle<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for WriteIdle<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.guard(cx, |inner, cx| Pin::new(inner).poll_write(cx, buf))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.guard(cx, |inner, cx| Pin::new(inner).poll_flush(cx))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.guard(cx, |inner, cx| Pin::new(inner).poll_shutdown(cx))
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.guard(cx, |inner, cx| {
            Pin::new(inner).poll_write_vectored(cx, bufs)
        })
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
    use tokio::time::Instant;

    const LIMIT: Duration = Duration::from_millis(100);

    #[tokio::test]
    async fn a_write_to_a_peer_that_never_reads_times_out() {
        // Arrange
        let (near, _far) = duplex(8);
        let mut socket = WriteIdle::new(near, LIMIT);
        let started = Instant::now();

        // Act
        let result = socket.write_all(&[0u8; 64]).await;

        // Assert
        let error = result.expect_err("the pipe holds 8 bytes and nobody reads");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= LIMIT);
    }

    #[tokio::test]
    async fn a_slow_but_reading_peer_keeps_the_connection() {
        // Arrange
        let (near, mut far) = duplex(8);
        let mut socket = WriteIdle::new(near, LIMIT);
        let reader = tokio::spawn(async move {
            let mut got = Vec::new();
            let mut chunk = [0u8; 8];
            while got.len() < 64 {
                tokio::time::sleep(LIMIT / 2).await;
                let n = far.read(&mut chunk).await.expect("read");
                got.extend_from_slice(&chunk[..n]);
            }
            got.len()
        });

        // Act
        let written = socket.write_all(&[7u8; 64]).await;

        // Assert
        written.expect("every write made progress inside the limit");
        assert_eq!(reader.await.expect("reader task"), 64);
    }
}
