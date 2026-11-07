//! `write_complete`: the whole object from a stream, one 2 MiB block at a time, synced through the barrier.

use std::future::poll_fn;
use std::sync::Arc;

use bytes::Bytes;
use contracts::{BlobError, BlobPlan, ByteStream, WrittenBlob};
use uuid::Uuid;

use super::FileBlobWriter;
use super::block::{BLOCK_LEN, OpenBlob, layout_of};

impl FileBlobWriter {
    pub(super) async fn write_whole(
        &self,
        _account: Uuid,
        size: i64,
        body: ByteStream,
    ) -> Result<WrittenBlob, BlobError> {
        layout_of(size)?;
        let (plan, key) = self.new_plan()?;
        let blob = Arc::new(self.create_blob(&plan, key, size, false).await?);
        match self.fill_and_sync(&blob, &plan, size, body).await {
            Ok(()) => Ok(WrittenBlob { plan, size }),
            Err(error) => {
                self.remove_file(plan.blob_id).await;
                Err(error)
            }
        }
    }

    async fn fill_and_sync(
        &self,
        blob: &Arc<OpenBlob>,
        plan: &BlobPlan,
        size: i64,
        mut body: ByteStream,
    ) -> Result<(), BlobError> {
        let declared = u64::try_from(size).map_err(|_| BlobError::OutOfRange)?;
        let mut pending: Vec<u8> = Vec::new();
        let mut received = 0_u64;
        let mut next_block = 0_u32;
        while let Some(item) = poll_fn(|cx| body.as_mut().poll_next(cx)).await {
            let bytes: Bytes = item.map_err(|error| BlobError::Io(Box::new(error)))?;
            received = received.saturating_add(bytes.len() as u64);
            if received > declared {
                return Err(BlobError::SizeMismatch);
            }
            let mut rest = &bytes[..];
            while !rest.is_empty() {
                let take = (BLOCK_LEN - pending.len()).min(rest.len());
                pending.extend_from_slice(&rest[..take]);
                rest = &rest[take..];
                if pending.len() == BLOCK_LEN {
                    self.put_block(blob, next_block, std::mem::take(&mut pending))
                        .await?;
                    next_block += 1;
                }
            }
        }
        if received != declared {
            return Err(BlobError::SizeMismatch);
        }
        if !pending.is_empty() || next_block == 0 {
            self.put_block(blob, next_block, pending).await?;
        }
        let token = self.barrier.register(plan.blob_id, Arc::clone(&blob.file));
        self.barrier.await_durable(token).await
    }

    async fn put_block(
        &self,
        blob: &Arc<OpenBlob>,
        block_index: u32,
        plaintext: Vec<u8>,
    ) -> Result<(), BlobError> {
        let blob = Arc::clone(blob);
        self.blocking(move || blob.write_block(block_index, &plaintext))
            .await
    }
}
