//! The `BlobWriter` port: planning without I/O, file creation under the free-space floor, encrypted block
//! writes acknowledged through the group-commit barrier, whole-object writes, and discard.
//!
//! The writer keeps no open files. A block write reopens the file, reads its nonce prefix and unwraps the data key
//! from the handle's wrapped key, so a handle works after a restart and from another request.

mod block;
mod complete;
mod volume;

use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
#[cfg(feature = "sync-hook")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use contracts::{
    BlobError, BlobHandle, BlobPlan, BlobWriter, ByteStream, DurableToken, WrittenBlob, async_trait,
};
use sc_observability::log_error;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::activity::{ActivityGauge, ActivityGuard};
use crate::barrier::SyncBarrier;
use crate::config::BlobstoreConfig;
use crate::data_key::DataKey;
use crate::error::{BlobIds, blob_error_from_io};
use crate::header::Header;
use crate::keyring::MasterKeyring;
use crate::path::blob_path;
use block::{OpenBlob, blob_ids, check_block, format_failure, layout_of};

pub struct FileBlobWriter {
    root: PathBuf,
    reserve: u64,
    preallocate: bool,
    keyring: Arc<MasterKeyring>,
    barrier: SyncBarrier,
    /// Caps the blocking jobs of the writer at `blob_io_threads`; the runtime's blocking pool is shared with
    /// the rest of the process and has no per-component limit.
    io: Arc<Semaphore>,
    /// The declared size of each blob between `plan` and `create_file`: the port's `create_file` takes the plan
    /// only, and the file length, the reserved space and the handle all depend on the size.
    planned: Mutex<HashMap<Uuid, i64>>,
    /// Counts the block writes in flight for the scrub's pause; absent in a process that has no scrub gauge.
    activity: Option<ActivityGauge>,
    #[cfg(feature = "sync-hook")]
    directory_syncs: AtomicU64,
}

impl FileBlobWriter {
    /// `config.blob_root` is required. `blob_io_threads` defaults to twice the cores.
    pub fn new(
        config: &BlobstoreConfig,
        keyring: Arc<MasterKeyring>,
        barrier: SyncBarrier,
    ) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), BlobIds::default())
            })?;
        let threads = config.blob_io_threads.map_or_else(
            || {
                std::thread::available_parallelism()
                    .map_or(2, |cores| cores.get().saturating_mul(2))
            },
            |threads| usize::try_from(threads).unwrap_or(usize::MAX),
        );
        Ok(FileBlobWriter {
            root: PathBuf::from(root),
            reserve: config.blob_free_reserve,
            preallocate: config.blob_preallocate,
            keyring,
            barrier,
            io: Arc::new(Semaphore::new(threads.clamp(1, Semaphore::MAX_PERMITS))),
            planned: Mutex::new(HashMap::new()),
            activity: None,
            #[cfg(feature = "sync-hook")]
            directory_syncs: AtomicU64::new(0),
        })
    }

    /// Every block write, from its first byte to its durability, counts in `gauge`.
    pub fn with_activity(mut self, gauge: ActivityGauge) -> Self {
        self.activity = Some(gauge);
        self
    }

    fn active(&self) -> Option<ActivityGuard> {
        self.activity.as_ref().map(ActivityGauge::enter)
    }

    /// Directory entries synced by completed file creations; the test hook of IT-04-5.
    #[cfg(feature = "sync-hook")]
    pub fn directory_syncs(&self) -> u64 {
        self.directory_syncs.load(Ordering::SeqCst)
    }

    fn new_plan(&self) -> Result<(BlobPlan, DataKey), BlobError> {
        let blob_id = Uuid::new_v4();
        let key = DataKey::generate().map_err(|error| format_failure(error, blob_ids(blob_id)))?;
        let (wrapped_key, master_key_id) = self.keyring.wrap_data_key(blob_id, &key)?;
        let plan = BlobPlan {
            blob_id,
            wrapped_key,
            master_key_id,
        };
        Ok((plan, key))
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, BlobError> + Send + 'static,
    ) -> Result<T, BlobError> {
        let permit = Arc::clone(&self.io)
            .acquire_owned()
            .await
            .map_err(|_| join_failure())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| join_failure())?
    }

    /// Floor check, then the file: header, reserved space, directory sync. The returned blob holds the open file.
    async fn create_blob(
        &self,
        plan: &BlobPlan,
        key: DataKey,
        size: i64,
        preallocate: bool,
    ) -> Result<OpenBlob, BlobError> {
        let layout = layout_of(size)?;
        let root = self.root.clone();
        let reserve = self.reserve;
        let preallocate = preallocate && self.preallocate;
        let blob_id = plan.blob_id;
        let blob = self
            .blocking(move || {
                let ids = blob_ids(blob_id);
                let needed = layout.file_len();
                let available = volume::available_bytes(&root)
                    .map_err(|error| blob_error_from_io(error, ids))?;
                volume::admit(available, needed, reserve)?;
                let header = Header::generate().map_err(|error| format_failure(error, ids))?;
                let file = volume::create_blob_file(&root, blob_id, &header, needed, preallocate)
                    .map_err(|error| blob_error_from_io(error, ids))?;
                Ok(OpenBlob::new(
                    Arc::new(file),
                    &key,
                    &header,
                    blob_id,
                    layout,
                ))
            })
            .await?;
        #[cfg(feature = "sync-hook")]
        self.directory_syncs.fetch_add(1, Ordering::SeqCst);
        Ok(blob)
    }

    fn planned_size(&self, blob_id: Uuid) -> Result<i64, BlobError> {
        self.planned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&blob_id)
            .copied()
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), blob_ids(blob_id))
            })
    }

    async fn remove_file(&self, blob_id: Uuid) {
        if let Err(error) = tokio::fs::remove_file(blob_path(&self.root, blob_id)).await
            && error.kind() != ErrorKind::NotFound
        {
            log_error!("partial blob file not removed", err = &error);
        }
    }
}

fn join_failure() -> BlobError {
    blob_error_from_io(
        io::Error::other("blocking task did not finish"),
        BlobIds::default(),
    )
}

#[async_trait]
impl BlobWriter for FileBlobWriter {
    fn plan(&self, _account: Uuid, plaintext_size: i64) -> Result<BlobPlan, BlobError> {
        layout_of(plaintext_size)?;
        let (plan, _key) = self.new_plan()?;
        self.planned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plan.blob_id, plaintext_size);
        Ok(plan)
    }

    async fn create_file(
        &self,
        plan: BlobPlan,
        preallocate: bool,
    ) -> Result<BlobHandle, BlobError> {
        let plaintext_size = self.planned_size(plan.blob_id)?;
        let key =
            self.keyring
                .unwrap_data_key(plan.blob_id, &plan.wrapped_key, plan.master_key_id)?;
        self.create_blob(&plan, key, plaintext_size, preallocate)
            .await?;
        self.planned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&plan.blob_id);
        Ok(BlobHandle {
            plan,
            plaintext_size,
        })
    }

    async fn open_for_write(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
    ) -> Result<BlobHandle, BlobError> {
        let layout = layout_of(plaintext_size)?;
        let plan = BlobPlan {
            blob_id,
            wrapped_key: wrapped_key.to_vec(),
            master_key_id,
        };
        let root = self.root.clone();
        let keyring = Arc::clone(&self.keyring);
        let probe = plan.clone();
        self.blocking(move || OpenBlob::open(&root, &keyring, &probe, layout).map(drop))
            .await?;
        Ok(BlobHandle {
            plan,
            plaintext_size,
        })
    }

    async fn write_block(
        &self,
        handle: &BlobHandle,
        block_index: u32,
        plaintext: Bytes,
    ) -> Result<DurableToken, BlobError> {
        let _active = self.active();
        let layout = layout_of(handle.plaintext_size)?;
        check_block(&layout, block_index, plaintext.len())?;
        let root = self.root.clone();
        let keyring = Arc::clone(&self.keyring);
        let plan = handle.plan.clone();
        let file = self
            .blocking(move || {
                let blob = OpenBlob::open(&root, &keyring, &plan, layout)?;
                blob.write_block(block_index, &plaintext)?;
                Ok(blob.file)
            })
            .await?;
        Ok(self.barrier.register(handle.plan.blob_id, file))
    }

    async fn await_durable(&self, token: DurableToken) -> Result<(), BlobError> {
        let _active = self.active();
        self.barrier.await_durable(token).await
    }

    async fn write_complete(
        &self,
        account: Uuid,
        size: i64,
        body: ByteStream,
    ) -> Result<WrittenBlob, BlobError> {
        self.write_whole(account, size, body).await
    }

    async fn discard(&self, blob_id: Uuid) -> Result<(), BlobError> {
        match tokio::fs::remove_file(blob_path(&self.root, blob_id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(blob_error_from_io(error, blob_ids(blob_id))),
        }
    }
}
