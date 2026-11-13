//! The port doubles of the `start_upload` and `put_block` tests, each at its boundary: a writer decorator that
//! delegates to the real 04 writer, can make file creation fail, counts block writes and can hold or fail the
//! group fdatasync wait; and the account guard reading the account state. The registry
//! is the real 04 one, through the port.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    AccountGuard, AccountGuardError, BlobError, BlobHandle, BlobPlan, BlobWriter, ByteStream,
    DurableToken, WrittenBlob, async_trait,
};
use sc_db::DbTx;
use sunnycloud_blobstore::FileBlobWriter;
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

/// How the next `create_file` calls end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateFault {
    None,
    Io,
    FreeSpaceFloor,
}

/// How the next `await_durable` calls end: delegated, held until [`FaultyWriter::release`] and then
/// delegated, or failed with `Io` without waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableFault {
    None,
    Hold,
    Io,
}

/// Delegates every call to the real writer; `create_file` fails as `fault` says, without touching the volume,
/// and `await_durable` as `durable` says.
pub struct FaultyWriter {
    inner: Arc<FileBlobWriter>,
    fault: Mutex<CreateFault>,
    durable: Mutex<DurableFault>,
    writes: AtomicUsize,
    parked: AtomicUsize,
    parked_changed: Notify,
    gate: Semaphore,
}

impl FaultyWriter {
    pub fn new(inner: FileBlobWriter) -> Self {
        FaultyWriter {
            inner: Arc::new(inner),
            fault: Mutex::new(CreateFault::None),
            durable: Mutex::new(DurableFault::None),
            writes: AtomicUsize::new(0),
            parked: AtomicUsize::new(0),
            parked_changed: Notify::new(),
            gate: Semaphore::new(0),
        }
    }

    pub fn set(&self, fault: CreateFault) {
        *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = fault;
    }

    pub fn set_durable(&self, fault: DurableFault) {
        *self.durable.lock().unwrap_or_else(PoisonError::into_inner) = fault;
    }

    /// `write_block` calls so far.
    pub fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }

    /// Returns once `count` calls are held in `await_durable` (in all, since the writer was built).
    pub async fn wait_parked(&self, count: usize) {
        loop {
            let changed = self.parked_changed.notified();
            if self.parked.load(Ordering::SeqCst) >= count {
                return;
            }
            changed.await;
        }
    }

    /// Lets every held and later held `await_durable` go on to the real one.
    pub fn release(&self) {
        self.gate.add_permits(Semaphore::MAX_PERMITS / 2);
    }
}

#[async_trait]
impl BlobWriter for FaultyWriter {
    fn plan(&self, account: Uuid, plaintext_size: i64) -> Result<BlobPlan, BlobError> {
        self.inner.plan(account, plaintext_size)
    }

    async fn create_file(
        &self,
        plan: BlobPlan,
        plaintext_size: i64,
        preallocate: bool,
    ) -> Result<BlobHandle, BlobError> {
        let fault = *self.fault.lock().unwrap_or_else(PoisonError::into_inner);
        match fault {
            CreateFault::None => {
                self.inner
                    .create_file(plan, plaintext_size, preallocate)
                    .await
            }
            CreateFault::Io => Err(BlobError::Io(Box::new(std::io::Error::other("injected")))),
            CreateFault::FreeSpaceFloor => Err(BlobError::FreeSpaceFloor),
        }
    }

    async fn open_for_write(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
    ) -> Result<BlobHandle, BlobError> {
        self.inner
            .open_for_write(blob_id, wrapped_key, master_key_id, plaintext_size)
            .await
    }

    async fn write_block(
        &self,
        handle: &BlobHandle,
        block_index: u32,
        plaintext: bytes::Bytes,
    ) -> Result<DurableToken, BlobError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.write_block(handle, block_index, plaintext).await
    }

    async fn await_durable(&self, token: DurableToken) -> Result<(), BlobError> {
        let fault = *self.durable.lock().unwrap_or_else(PoisonError::into_inner);
        match fault {
            DurableFault::None => {}
            DurableFault::Io => {
                return Err(BlobError::Io(Box::new(std::io::Error::other("injected"))));
            }
            DurableFault::Hold => {
                self.parked.fetch_add(1, Ordering::SeqCst);
                self.parked_changed.notify_waiters();
                drop(self.gate.acquire().await.expect("the gate is never closed"));
            }
        }
        self.inner.await_durable(token).await
    }

    async fn write_complete(
        &self,
        account: Uuid,
        size: i64,
        body: ByteStream,
    ) -> Result<WrittenBlob, BlobError> {
        self.inner.write_complete(account, size, body).await
    }

    async fn discard(&self, blob_id: Uuid) -> Result<(), BlobError> {
        self.inner.discard(blob_id).await
    }
}

/// The `AccountGuard` port backed at its boundary: the account state read `FOR SHARE`, as the identity guard does.
pub struct StateGuard;

#[async_trait]
impl AccountGuard for StateGuard {
    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountGuardError> {
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM accounts WHERE id = $1 FOR SHARE")
                .bind(account)
                .fetch_optional(tx.conn())
                .await
                .map_err(|error| AccountGuardError::Unexpected(Box::new(error)))?;
        match state.as_deref() {
            Some("active") => Ok(()),
            _ => Err(AccountGuardError::AccountNotActive),
        }
    }
}
