//! The plaintext stream: the authenticated first chunk, then the chunks a background task reads ahead.

use std::io::{self, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{BlobError, StreamError};
use futures_core::Stream;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::file::{OpenedBlob, blob_ids};
use super::finding::Findings;
use super::probe::ReadProbe;
use crate::activity::ActivityGuard;
use crate::error::blob_error_from_io;

type Item = Result<Bytes, StreamError>;

pub(super) fn stream_error(error: BlobError) -> StreamError {
    match error {
        BlobError::Corrupt => StreamError::Corrupt,
        BlobError::Io(fault) => StreamError::Source(fault),
        other => StreamError::Source(Box::new(other)),
    }
}

/// Yields the first chunk, then what the producer sends. `remaining` is what the range still owes: a channel that
/// closes before it is paid (the producer task was lost) is an error item, never a normal end.
pub(super) struct ChunkStream {
    head: Option<Bytes>,
    rx: Option<mpsc::Receiver<Item>>,
    remaining: u64,
    blob_id: Uuid,
    probe: Arc<ReadProbe>,
    _active: Option<ActivityGuard>,
}

impl ChunkStream {
    pub fn new(
        head: Bytes,
        rx: Option<mpsc::Receiver<Item>>,
        remaining: u64,
        blob_id: Uuid,
        probe: Arc<ReadProbe>,
        active: Option<ActivityGuard>,
    ) -> Self {
        ChunkStream {
            head: (!head.is_empty()).then_some(head),
            rx,
            remaining,
            blob_id,
            probe,
            _active: active,
        }
    }
}

impl Stream for ChunkStream {
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Item>> {
        let this = self.get_mut();
        if let Some(head) = this.head.take() {
            return Poll::Ready(Some(Ok(head)));
        }
        let Some(rx) = this.rx.as_mut() else {
            return Poll::Ready(None);
        };
        match rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(bytes))) => {
                this.probe.slot_released();
                this.remaining = this.remaining.saturating_sub(bytes.len() as u64);
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.probe.slot_released();
                this.rx = None;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.rx = None;
                if this.remaining == 0 {
                    return Poll::Ready(None);
                }
                let lost = blob_error_from_io(
                    io::Error::from(ErrorKind::UnexpectedEof),
                    blob_ids(this.blob_id),
                );
                Poll::Ready(Some(Err(stream_error(lost))))
            }
        }
    }
}

/// The chunks after the first, read and verified one at a time, at most the window ahead of the consumer.
pub(super) struct ReadAhead {
    pub blob: Arc<OpenedBlob>,
    pub findings: Arc<Findings>,
    pub probe: Arc<ReadProbe>,
    pub blob_id: Uuid,
    pub first: u64,
    pub last: u64,
    pub remaining: u64,
}

impl ReadAhead {
    pub fn start(self, window: usize) -> mpsc::Receiver<Item> {
        let (tx, rx) = mpsc::channel(window);
        tokio::spawn(self.run(tx));
        rx
    }

    /// A slot is reserved before a chunk is read, so the window bounds chunks in flight as well as queued ones. A
    /// dropped consumer closes the channel and ends the task at its next reservation.
    async fn run(self, tx: mpsc::Sender<Item>) {
        let mut remaining = self.remaining;
        for index in self.first..=self.last {
            let Ok(slot) = tx.reserve().await else {
                return;
            };
            self.probe.slot_reserved();
            let blob = Arc::clone(&self.blob);
            let read = tokio::task::spawn_blocking(move || blob.read_chunk(index)).await;
            let outcome = read
                .map_err(|_| join_failure(self.blob_id))
                .and_then(|chunk| chunk);
            match outcome {
                Ok(plain) => {
                    let mut bytes = Bytes::from(plain);
                    bytes.truncate(usize::try_from(remaining).unwrap_or(usize::MAX));
                    remaining -= bytes.len() as u64;
                    slot.send(Ok(bytes));
                }
                Err(error) => {
                    self.findings.note(self.blob_id, &error).await;
                    slot.send(Err(stream_error(error)));
                    return;
                }
            }
        }
    }
}

pub(super) fn join_failure(blob_id: Uuid) -> BlobError {
    blob_error_from_io(
        io::Error::other("blocking task did not finish"),
        blob_ids(blob_id),
    )
}
