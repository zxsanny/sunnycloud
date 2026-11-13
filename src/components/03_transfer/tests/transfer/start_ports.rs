//! The port doubles of the `start_upload` tests, each at its boundary: a writer decorator that delegates to the
//! real 04 writer and can make file creation fail, the registry port over the real 04 registry, and the account
//! guard reading the account state.

use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    AccountGuard, AccountGuardError, BlobError, BlobHandle, BlobPlan, BlobRegistryError,
    BlobWriter, ByteStream, DurableToken, RegisterState, ReinstateOutcome, WrittenBlob,
    WrittenOrPlanned, async_trait,
};
use sc_db::DbTx;
use sunnycloud_blobstore::{BlobRegistry, FileBlobWriter};
use uuid::Uuid;

/// How the next `create_file` calls end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateFault {
    None,
    Io,
    FreeSpaceFloor,
}

/// Delegates every call to the real writer; `create_file` fails as `fault` says, without touching the volume.
pub struct FaultyWriter {
    inner: Arc<FileBlobWriter>,
    fault: Mutex<CreateFault>,
}

impl FaultyWriter {
    pub fn new(inner: FileBlobWriter) -> Self {
        FaultyWriter {
            inner: Arc::new(inner),
            fault: Mutex::new(CreateFault::None),
        }
    }

    pub fn set(&self, fault: CreateFault) {
        *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = fault;
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
        preallocate: bool,
    ) -> Result<BlobHandle, BlobError> {
        let fault = *self.fault.lock().unwrap_or_else(PoisonError::into_inner);
        match fault {
            CreateFault::None => self.inner.create_file(plan, preallocate).await,
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
        self.inner.write_block(handle, block_index, plaintext).await
    }

    async fn await_durable(&self, token: DurableToken) -> Result<(), BlobError> {
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

/// The `BlobRegistry` port over the real 04 registry for one account. The port's `register_in_tx` carries no
/// account while 04's takes one, so the adapter supplies the rig's account; every statement is 04's own.
pub struct AccountRegistry {
    inner: BlobRegistry,
    account: Uuid,
}

impl AccountRegistry {
    pub fn new(inner: BlobRegistry, account: Uuid) -> Self {
        AccountRegistry { inner, account }
    }
}

#[async_trait]
impl contracts::BlobRegistry for AccountRegistry {
    async fn register_in_tx(
        &self,
        tx: &mut DbTx,
        blob: WrittenOrPlanned,
        state: RegisterState,
    ) -> Result<(), BlobRegistryError> {
        self.inner
            .register_in_tx(tx, self.account, blob, state)
            .await
    }

    async fn mark_committed_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
        size: i64,
    ) -> Result<(), BlobRegistryError> {
        self.inner.mark_committed_in_tx(tx, blob_id, size).await
    }

    async fn reinstate_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<ReinstateOutcome, BlobRegistryError> {
        self.inner.reinstate_in_tx(tx, blob_id).await
    }

    async fn remove_row_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<(), BlobRegistryError> {
        self.inner.remove_row_in_tx(tx, blob_id).await
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
