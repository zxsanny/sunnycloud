//! The plaintext stream: the authenticated first chunk, then the chunks a background task reads ahead in a window
//! that widens while the consumer keeps pace.

use std::io::{self, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{BlobError, StreamError};
use futures_core::Stream;
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, mpsc};
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

/// Yields the first chunk, then what the read-ahead task sends. `remaining` is what the range still owes: a channel
/// that closes before it is paid (the producer task was lost) is an error item, never a normal end.
///
/// The first chunk is the whole window at the start: the read-ahead task is spawned only when that chunk is taken,
/// so a stream nobody polls holds one chunk. The window then starts at one chunk ahead and doubles, up to
/// `blob_read_ahead_chunks`, each time the consumer polls and finds nothing ready (it keeps pace with the disk); a
/// consumer that is slower than the disk finds its next chunk waiting and never widens it.
pub(super) struct ChunkStream {
    head: Option<Bytes>,
    ahead: Option<Pending>,
    flow: Option<Flow>,
    remaining: u64,
    blob_id: Uuid,
    probe: Arc<ReadProbe>,
    _active: Option<ActivityGuard>,
}

/// The read-ahead task before it runs, with the runtime it will run on and the widest window it may reach.
pub(super) struct Pending {
    pub task: ReadAhead,
    pub runtime: Handle,
    pub max_window: usize,
}

/// A running read-ahead: its channel and its credit. The task takes one permit of `credit` before it reads a chunk
/// and the consumer gives it back when it takes that chunk, so `window` bounds chunks in flight and queued.
struct Flow {
    rx: mpsc::UnboundedReceiver<Item>,
    credit: Arc<Semaphore>,
    window: usize,
    max_window: usize,
    /// Set when the window widened since the consumer last took a chunk, so a spurious second poll does not
    /// widen it again.
    widened: bool,
}

impl Flow {
    fn start(pending: Pending) -> Flow {
        let (tx, rx) = mpsc::unbounded_channel();
        let credit = Arc::new(Semaphore::new(1));
        pending
            .runtime
            .spawn(pending.task.run(Arc::clone(&credit), tx));
        Flow {
            rx,
            credit,
            window: 1,
            max_window: pending.max_window,
            widened: false,
        }
    }

    fn took_chunk(&mut self) {
        self.credit.add_permits(1);
        self.widened = false;
    }

    /// The consumer waits for the disk: one doubling per chunk taken, never past the configured window.
    fn consumer_waits(&mut self, probe: &ReadProbe) {
        if self.widened || self.window >= self.max_window {
            return;
        }
        let grow = self.window.min(self.max_window - self.window);
        self.credit.add_permits(grow);
        self.window += grow;
        self.widened = true;
        probe.window_widened(self.window);
    }
}

impl ChunkStream {
    pub fn new(
        head: Bytes,
        ahead: Option<Pending>,
        remaining: u64,
        blob_id: Uuid,
        probe: Arc<ReadProbe>,
        active: Option<ActivityGuard>,
    ) -> Self {
        ChunkStream {
            head: (!head.is_empty()).then_some(head),
            ahead,
            flow: None,
            remaining,
            blob_id,
            probe,
            _active: active,
        }
    }
}

impl Drop for ChunkStream {
    /// A task waiting for credit would wait forever once the consumer is gone; closing the credit ends it.
    fn drop(&mut self) {
        if let Some(flow) = &self.flow {
            flow.credit.close();
        }
    }
}

impl Stream for ChunkStream {
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Item>> {
        let this = self.get_mut();
        if let Some(pending) = this.ahead.take() {
            this.flow = Some(Flow::start(pending));
        }
        if let Some(head) = this.head.take() {
            return Poll::Ready(Some(Ok(head)));
        }
        let Some(flow) = this.flow.as_mut() else {
            return Poll::Ready(None);
        };
        match flow.rx.poll_recv(cx) {
            Poll::Pending => {
                if this.remaining > 0 {
                    flow.consumer_waits(&this.probe);
                }
                Poll::Pending
            }
            Poll::Ready(Some(Ok(bytes))) => {
                this.probe.slot_released();
                flow.took_chunk();
                this.remaining = this.remaining.saturating_sub(bytes.len() as u64);
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.probe.slot_released();
                flow.credit.close();
                this.flow = None;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.flow = None;
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

/// The chunks after the first, read and verified one at a time, each only once the window has room for it.
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
    /// A permit is taken before a chunk is read, so the window bounds chunks in flight as well as queued ones. A
    /// dropped consumer closes the credit and ends the task at its next permit, or the channel at its next send.
    async fn run(self, credit: Arc<Semaphore>, tx: mpsc::UnboundedSender<Item>) {
        let mut remaining = self.remaining;
        for index in self.first..=self.last {
            let Ok(permit) = credit.acquire().await else {
                return;
            };
            permit.forget();
            self.probe.slot_reserved();
            let blob = Arc::clone(&self.blob);
            let read = tokio::task::spawn_blocking(move || blob.read_chunk(index)).await;
            let outcome = read
                .map_err(|_| join_failure(self.blob_id))
                .and_then(|chunk| chunk);
            let item = match outcome {
                Ok(plain) => {
                    let mut bytes = Bytes::from(plain);
                    bytes.truncate(usize::try_from(remaining).unwrap_or(usize::MAX));
                    remaining -= bytes.len() as u64;
                    Ok(bytes)
                }
                Err(error) => {
                    self.findings.note(self.blob_id, &error).await;
                    Err(stream_error(error))
                }
            };
            let last_item = item.is_err();
            // A failed send means the consumer is gone; a failed read is recorded above either way.
            if tx.send(item).is_err() || last_item {
                return;
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
