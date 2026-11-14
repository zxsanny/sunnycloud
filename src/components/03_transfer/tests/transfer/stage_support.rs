//! Shared pieces of the `stage_small` tests: a stage call as the client makes it (through the `UploadService`
//! port), a body that stalls after its first poll, and probes of the staged row.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{ByteStream, Staged, StreamError, UploadError, UploadService};
use futures_core::Stream;
use sc_ids::{Hash32, hash_block};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::put_support::body;
use crate::start_support::{Rig, close};

/// `blob_id, declared_size, block_count, reserved_bytes, received, block_hashes`.
type RowColumns = (Uuid, i64, i32, i64, Vec<u8>, Vec<u8>);

/// A staged row as the tests read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRow {
    pub blob: Uuid,
    pub declared_size: i64,
    pub block_count: i32,
    pub reserved_bytes: i64,
    pub received: Vec<u8>,
    pub block_hashes: Vec<u8>,
}

impl Rig {
    /// Stages `data` under `key` with the client's hash (none for an empty file).
    pub async fn stage(&self, key: Uuid, data: &[u8]) -> Result<Staged, UploadError> {
        let hash = (!data.is_empty()).then(|| hash_block(data));
        self.stage_raw(key, hash, data, data.len() as u64).await.0
    }

    /// Stages `bytes` declared as `length` with `hash`; returns the answer and the body bytes read.
    pub async fn stage_raw(
        &self,
        key: Uuid,
        hash: Option<Hash32>,
        bytes: &[u8],
        length: u64,
    ) -> (Result<Staged, UploadError>, usize) {
        let (stream, taken) = body(bytes, 256 * 1024);
        let service: &dyn UploadService = self.service.as_ref();
        let result = service
            .stage_small(self.account, key, hash, stream, length)
            .await;
        (result, taken.load(Ordering::SeqCst))
    }

    pub async fn staged_row(&self, key: Uuid) -> Option<StagedRow> {
        let mut conn = self.owner().await;
        let row: Option<RowColumns> = sqlx::query_as(
            "SELECT blob_id, declared_size, block_count, reserved_bytes, received, block_hashes \
             FROM upload_sessions WHERE account_id = $1 AND client_key = $2",
        )
        .bind(self.account)
        .bind(key.to_string())
        .fetch_optional(&mut conn)
        .await
        .expect("staged row");
        close(conn).await;
        row.map(
            |(blob, declared_size, block_count, reserved_bytes, received, block_hashes)| {
                StagedRow {
                    blob,
                    declared_size,
                    block_count,
                    reserved_bytes,
                    received,
                    block_hashes,
                }
            },
        )
    }

    /// `count` staged rows of 1 byte each (with their `uploading` blob rows and no files), written directly.
    pub async fn fill_staged(&self, count: i64) {
        self.exec(&format!(
            "WITH b AS (INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
             SELECT gen_random_uuid(), '{account}', 'uploading', '\\x00', 1, 'epoch'::timestamptz \
             FROM generate_series(1, {count}) RETURNING id) \
             INSERT INTO upload_sessions (id, account_id, blob_id, client_key, declared_size, block_count, \
             reserved_bytes, received, block_hashes, created_at, last_checkpoint_at, expires_at) \
             SELECT gen_random_uuid(), '{account}', id, id::text, 1, 1, 1, '\\x01', \
             decode(repeat('00', 32), 'hex'), 'epoch'::timestamptz, 'epoch'::timestamptz, \
             '2100-01-01'::timestamptz FROM b",
            account = self.account
        ))
        .await;
    }
}

/// A body that announces its first poll and then never yields: a request still being received.
pub struct Stalled {
    polled: Arc<Notify>,
}

impl Stream for Stalled {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.polled.notify_one();
        Poll::Pending
    }
}

/// A stalled body and the signal of its first poll.
pub fn stalled() -> (ByteStream, Arc<Notify>) {
    let polled = Arc::new(Notify::new());
    let stream = Stalled {
        polled: Arc::clone(&polled),
    };
    (Box::pin(stream), polled)
}
