//! Shared pieces of the `put_block` tests: request bodies as hand-written streams that count the bytes handed
//! out, sessions with deterministic content, and probes of the session row and the blob file.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{BlockAck, ByteStream, StreamError, UploadError};
use futures_core::Stream;
use sc_ids::{BLOCK_SIZE, Hash32, hash_block};
use uuid::Uuid;

use crate::start_support::{MIB, Rig, request, started};

/// A body of fixed pieces; `taken` counts the bytes the service pulled out of it.
struct Pieces {
    items: VecDeque<Bytes>,
    taken: Arc<AtomicUsize>,
}

impl Stream for Pieces {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.items.pop_front();
        if let Some(piece) = &next {
            self.taken.fetch_add(piece.len(), Ordering::SeqCst);
        }
        Poll::Ready(next.map(Ok))
    }
}

/// `bytes` in pieces of `piece` bytes, and the counter of the bytes read from it.
pub fn body(bytes: &[u8], piece: usize) -> (ByteStream, Arc<AtomicUsize>) {
    let taken = Arc::new(AtomicUsize::new(0));
    let items = bytes.chunks(piece).map(Bytes::copy_from_slice).collect();
    let stream = Pieces {
        items,
        taken: Arc::clone(&taken),
    };
    (Box::pin(stream), taken)
}

/// Deterministic content of `len` bytes for `seed`.
pub fn content(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

/// A started session, its blob and the content the client will send.
pub struct Upload {
    pub id: Uuid,
    pub blob: Uuid,
    pub data: Vec<u8>,
}

impl Upload {
    pub fn block(&self, index: u32) -> &[u8] {
        let start = index as usize * BLOCK_SIZE;
        let end = (start + BLOCK_SIZE).min(self.data.len());
        &self.data[start..end]
    }

    pub fn hash(&self, index: u32) -> Hash32 {
        hash_block(self.block(index))
    }

    pub fn block_count(&self) -> u32 {
        u32::try_from(self.data.len().div_ceil(BLOCK_SIZE)).expect("block count")
    }
}

impl Rig {
    /// Starts a session of `size` bytes with content from `seed`.
    pub async fn upload(&self, size: usize, seed: u64) -> Upload {
        let key = format!("put-{seed}-{size}");
        let declared = i64::try_from(size).expect("size");
        let id = started(self.start(request(self.root, &key, declared, &key)).await).upload_id;
        let blob = self.blob_of(id).await;
        Upload {
            id,
            blob,
            data: content(seed, size),
        }
    }

    /// Sends block `index` of `upload` as the client would: its bytes, hash and length.
    pub async fn put(&self, upload: &Upload, index: u32) -> Result<BlockAck, UploadError> {
        let bytes = upload.block(index);
        self.put_raw(
            upload.id,
            index,
            upload.hash(index),
            bytes,
            bytes.len() as u64,
        )
        .await
        .0
    }

    /// Sends `bytes` declared as `length` with `hash`; returns the answer and the body bytes read.
    pub async fn put_raw(
        &self,
        upload_id: Uuid,
        index: u32,
        hash: Hash32,
        bytes: &[u8],
        length: u64,
    ) -> (Result<BlockAck, UploadError>, usize) {
        let (stream, taken) = body(bytes, 256 * 1024);
        let result = self
            .service
            .put_block(self.account, upload_id, index, hash, stream, length)
            .await;
        (result, taken.load(Ordering::SeqCst))
    }

    pub async fn blob_of(&self, upload_id: Uuid) -> Uuid {
        let mut conn = self.owner().await;
        let blob = sqlx::query_scalar(
            "SELECT blob_id FROM upload_sessions WHERE id = $1 AND account_id = $2",
        )
        .bind(upload_id)
        .bind(self.account)
        .fetch_one(&mut conn)
        .await
        .expect("session blob");
        crate::start_support::close(conn).await;
        blob
    }

    /// `received`, `block_hashes` and `expires_at` (Unix microseconds) of the session row.
    pub async fn row(&self, upload_id: Uuid) -> (Vec<u8>, Vec<u8>, i64) {
        let mut conn = self.owner().await;
        let row = sqlx::query_as(
            "SELECT received, block_hashes, (extract(epoch FROM expires_at) * 1000000)::bigint \
             FROM upload_sessions WHERE id = $1 AND account_id = $2",
        )
        .bind(upload_id)
        .bind(self.account)
        .fetch_one(&mut conn)
        .await
        .expect("session row");
        crate::start_support::close(conn).await;
        row
    }

    /// The raw (encrypted) blob file.
    pub fn file_bytes(&self, blob: Uuid) -> Vec<u8> {
        std::fs::read(sunnycloud_blobstore::blob_path(&self.volume, blob)).expect("blob file")
    }

    /// The plaintext of the blob, read back through the real 04 reader.
    pub async fn read_back(&self, upload: &Upload) -> Vec<u8> {
        use contracts::BlobReader;
        let mut conn = self.owner().await;
        let (wrapped_key, master_key_id): (Vec<u8>, i16) = sqlx::query_as(
            "SELECT wrapped_key, master_key_id FROM blobs WHERE id = $1 AND account_id = $2",
        )
        .bind(upload.blob)
        .bind(self.account)
        .fetch_one(&mut conn)
        .await
        .expect("blob key");
        crate::start_support::close(conn).await;
        let size = i64::try_from(upload.data.len()).expect("size");
        let mut stream = self
            .reader
            .open(upload.blob, &wrapped_key, master_key_id, size, None)
            .await
            .expect("open blob");
        let mut plain = Vec::with_capacity(upload.data.len());
        while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            plain.extend_from_slice(&chunk.expect("authentic chunk"));
        }
        plain
    }
}

/// Bit `index` of a `received` bitmap.
pub fn bit(received: &[u8], index: u32) -> bool {
    received[index as usize / 8] & (1 << (index % 8)) != 0
}

pub const NINE_MIB: usize = 9 * MIB as usize;

/// Sends block `index` of `upload` on its own task, as one lane of a client.
pub fn spawn_put(
    rig: &Rig,
    upload: &Upload,
    index: u32,
) -> tokio::task::JoinHandle<Result<BlockAck, UploadError>> {
    let service = Arc::clone(&rig.service);
    let account = rig.account;
    let id = upload.id;
    let bytes = upload.block(index).to_vec();
    let hash = upload.hash(index);
    tokio::spawn(async move {
        let (stream, _) = body(&bytes, 256 * 1024);
        let length = bytes.len() as u64;
        service
            .put_block(account, id, index, hash, stream, length)
            .await
    })
}
