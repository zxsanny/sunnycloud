//! Recording doubles of the 04 blobstore ports (AC-2) and the shapes of their DTOs and errors.

mod common;

use std::sync::Arc;

use bytes::Bytes;
use common::{Calls, NOW, id, sorted};
use contracts::{
    BlobCatalog, BlobCatalogError, BlobError, BlobHandle, BlobInventory, BlobPlan, BlobReader,
    BlobRef, BlobRegistry, BlobRegistryError, BlobWriter, ByteRange, ByteRate, ByteStream, Due,
    DurableToken, EpochMarker, EpochMarkerError, GcQueue, GcQueueError, GcReason, InventoryError,
    KeyError, Keyring, MasterKey, MasterKeyring, PlaintextStream, RegisterState, ReinstateOutcome,
    ScrubError, ScrubMode, ScrubRun, ScrubStatus, ScrubSummary, Scrubber, StepResult, StreamError,
    WrittenBlob, WrittenOrPlanned, async_trait,
};
use futures_core::Stream;
use sc_db::DbTx;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

struct Empty;

impl Stream for Empty {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

fn plan() -> BlobPlan {
    BlobPlan {
        blob_id: id(),
        wrapped_key: vec![222, 173, 190, 239],
        master_key_id: 1,
    }
}

fn handle() -> BlobHandle {
    BlobHandle {
        plan: plan(),
        plaintext_size: 10,
    }
}

#[async_trait]
impl BlobWriter for Double {
    fn plan(&self, _account: Uuid, _plaintext_size: i64) -> Result<BlobPlan, BlobError> {
        self.0.record("plan");
        Ok(plan())
    }

    async fn create_file(
        &self,
        _plan: BlobPlan,
        _plaintext_size: i64,
        _preallocate: bool,
    ) -> Result<BlobHandle, BlobError> {
        self.0.record("create_file");
        Ok(handle())
    }

    async fn open_for_write(
        &self,
        _blob_id: Uuid,
        _wrapped_key: &[u8],
        _master_key_id: i16,
        _plaintext_size: i64,
    ) -> Result<BlobHandle, BlobError> {
        self.0.record("open_for_write");
        Ok(handle())
    }

    async fn write_block(
        &self,
        _handle: &BlobHandle,
        block_index: u32,
        _plaintext: Bytes,
    ) -> Result<DurableToken, BlobError> {
        self.0.record("write_block");
        Ok(DurableToken(u64::from(block_index)))
    }

    async fn await_durable(&self, _token: DurableToken) -> Result<(), BlobError> {
        self.0.record("await_durable");
        Ok(())
    }

    async fn write_complete(
        &self,
        _account: Uuid,
        size: i64,
        _body: ByteStream,
    ) -> Result<WrittenBlob, BlobError> {
        self.0.record("write_complete");
        Ok(WrittenBlob { plan: plan(), size })
    }

    async fn discard(&self, _blob_id: Uuid) -> Result<(), BlobError> {
        self.0.record("discard");
        Ok(())
    }
}

#[async_trait]
impl BlobReader for Double {
    async fn open(
        &self,
        _blob_id: Uuid,
        _wrapped_key: &[u8],
        _master_key_id: i16,
        _plaintext_size: i64,
        range: Option<ByteRange>,
    ) -> Result<PlaintextStream, BlobError> {
        self.0.record("open");
        if range.is_some_and(|r| r.start > r.end) {
            return Err(BlobError::RangeNotSatisfiable);
        }
        Ok(Box::pin(Empty))
    }

    async fn read_head(
        &self,
        _blob_id: Uuid,
        _wrapped_key: &[u8],
        _master_key_id: i16,
        _plaintext_size: i64,
        _max_bytes: u32,
    ) -> Result<Bytes, BlobError> {
        self.0.record("read_head");
        Ok(Bytes::from_static(b"head"))
    }
}

#[async_trait]
impl BlobRegistry for Double {
    async fn register_in_tx(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _blob: WrittenOrPlanned,
        _state: RegisterState,
    ) -> Result<(), BlobRegistryError> {
        self.0.record("register_in_tx");
        Ok(())
    }

    async fn mark_committed_in_tx(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _blob_id: Uuid,
        _size: i64,
    ) -> Result<(), BlobRegistryError> {
        self.0.record("mark_committed_in_tx");
        Ok(())
    }

    async fn reinstate_in_tx(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _blob_id: Uuid,
    ) -> Result<ReinstateOutcome, BlobRegistryError> {
        self.0.record("reinstate_in_tx");
        Ok(ReinstateOutcome::Reinstated)
    }

    async fn remove_row_in_tx(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _blob_id: Uuid,
    ) -> Result<(), BlobRegistryError> {
        self.0.record("remove_row_in_tx");
        Ok(())
    }
}

#[async_trait]
impl BlobCatalog for Double {
    async fn get_ref(
        &self,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<Option<BlobRef>, BlobCatalogError> {
        self.0.record("get_ref");
        if account != id() {
            return Ok(None);
        }
        Ok(Some(BlobRef {
            blob_id,
            size: 10,
            wrapped_key: vec![1],
            master_key_id: 1,
        }))
    }
}

#[async_trait]
impl GcQueue for Double {
    async fn schedule_in_tx(
        &self,
        _tx: &mut DbTx,
        _blob_id: Uuid,
        _reason: GcReason,
        _due: Due,
    ) -> Result<(), GcQueueError> {
        self.0.record("schedule_in_tx");
        Ok(())
    }

    async fn pending_for_account(&self, _account: Uuid) -> Result<u64, GcQueueError> {
        self.0.record("pending_for_account");
        Ok(3)
    }
}

#[async_trait]
impl Scrubber for Double {
    async fn start_run(
        &self,
        _mode: ScrubMode,
        _rate: Option<ByteRate>,
    ) -> Result<ScrubRun, ScrubError> {
        self.0.record("start_run");
        Ok(ScrubRun { started_at: NOW })
    }

    async fn run_step(&self, _run: &ScrubRun) -> Result<StepResult, ScrubError> {
        self.0.record("run_step");
        Ok(StepResult {
            checked: 5,
            mismatched: 1,
            missing: 0,
            done: true,
        })
    }

    async fn finish_run(&self, _run: &ScrubRun) -> Result<(), ScrubError> {
        self.0.record("finish_run");
        Ok(())
    }

    async fn status(&self) -> Result<ScrubStatus, ScrubError> {
        self.0.record("status");
        Ok(ScrubStatus {
            oldest_scrubbed_at: Some(NOW),
            unscrubbed: 2,
            latest_summary: Some(ScrubSummary {
                checked: 5,
                mismatched: 1,
                missing: 0,
                duration: Duration::from_secs(3),
            }),
            unavailable: 1,
        })
    }
}

#[async_trait]
impl BlobInventory for Double {
    async fn find_missing(&self, _batch: u32) -> Result<Vec<Uuid>, InventoryError> {
        self.0.record("find_missing");
        Ok(vec![id()])
    }
}

#[async_trait]
impl MasterKeyring for Double {
    async fn load_and_verify(&self) -> Result<Keyring, KeyError> {
        self.0.record("load_and_verify");
        Ok(Keyring {
            current_key_id: 2,
            key_ids: vec![1, 2],
        })
    }

    async fn init_key_check(&self, candidate: &MasterKey) -> Result<(), KeyError> {
        self.0.record("init_key_check");
        if candidate.0.len() != 32 {
            return Err(KeyError::WrongLength);
        }
        Ok(())
    }

    async fn rewrap_batch(&self, _from_id: i16, _to_id: i16, n: u32) -> Result<u64, KeyError> {
        self.0.record("rewrap_batch");
        Ok(u64::from(n))
    }
}

#[async_trait]
impl EpochMarker for Double {
    async fn write(&self, _epoch: i64) -> Result<(), EpochMarkerError> {
        self.0.record("write");
        Ok(())
    }

    async fn read(&self) -> Result<Option<i64>, EpochMarkerError> {
        self.0.record("read");
        Ok(None)
    }
}

#[tokio::test]
async fn blob_writer_double_records_every_method() {
    // Arrange
    let calls = Calls::default();
    let writer: Arc<dyn BlobWriter> = Arc::new(Double(calls.clone()));
    let body: ByteStream = Box::pin(Empty);

    // Act
    let planned = writer.plan(id(), 10).expect("plan");
    let opened = writer.create_file(planned, 10, true).await.expect("create");
    writer
        .open_for_write(id(), &[1], 1, 10)
        .await
        .expect("reopen");
    let token = writer
        .write_block(&opened, 3, Bytes::from_static(b"x"))
        .await
        .expect("write");
    writer.await_durable(token).await.expect("durable");
    let written = writer.write_complete(id(), 0, body).await.expect("whole");
    writer.discard(written.plan.blob_id).await.expect("discard");

    // Assert
    assert_eq!(token, DurableToken(3));
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "plan",
            "create_file",
            "open_for_write",
            "write_block",
            "await_durable",
            "write_complete",
            "discard"
        ])
    );
}

#[tokio::test]
async fn blob_reader_double_opens_a_range_and_reads_the_head() {
    // Arrange
    let calls = Calls::default();
    let reader: Arc<dyn BlobReader> = Arc::new(Double(calls.clone()));

    // Act
    let stream = reader
        .open(id(), &[1], 1, 10, Some(ByteRange { start: 0, end: 4 }))
        .await;
    let head = reader.read_head(id(), &[1], 1, 10, 65_536).await;

    // Assert
    assert!(stream.is_ok());
    assert_eq!(head.expect("head"), Bytes::from_static(b"head"));
    assert_eq!(calls.sorted(), sorted(vec!["open", "read_head"]));
}

#[tokio::test]
async fn blob_reader_open_with_an_inverted_range_fails_with_range_not_satisfiable() {
    // Arrange
    let reader: Arc<dyn BlobReader> = Arc::new(Double::default());

    // Act
    let outcome = reader
        .open(id(), &[1], 1, 10, Some(ByteRange { start: 5, end: 1 }))
        .await;

    // Assert
    assert!(matches!(outcome, Err(BlobError::RangeNotSatisfiable)));
}

#[test]
fn blob_registry_double_is_held_as_a_trait_object() {
    // Arrange
    let calls = Calls::default();

    // Act: the methods need a `DbTx`, which only a live database hands out; see `tx_join.rs`
    let registry: Arc<dyn BlobRegistry> = Arc::new(Double(calls.clone()));

    // Assert
    assert_eq!(Arc::strong_count(&registry), 1);
    assert!(calls.sorted().is_empty());
}

#[tokio::test]
async fn blob_catalog_hides_another_accounts_blob() {
    // Arrange
    let calls = Calls::default();
    let catalog: Arc<dyn BlobCatalog> = Arc::new(Double(calls.clone()));

    // Act
    let own = catalog.get_ref(id(), id()).await.expect("own");
    let foreign = catalog
        .get_ref(Uuid::from_u128(9), id())
        .await
        .expect("foreign");

    // Assert
    assert!(own.is_some());
    assert!(foreign.is_none());
    assert_eq!(calls.sorted(), sorted(vec!["get_ref", "get_ref"]));
}

#[tokio::test]
async fn gc_queue_double_counts_pending_blobs_and_is_held_as_a_trait_object() {
    // Arrange
    let calls = Calls::default();
    let queue: Arc<dyn GcQueue> = Arc::new(Double(calls.clone()));

    // Act
    let pending = queue.pending_for_account(id()).await.expect("pending");

    // Assert
    assert_eq!(pending, 3);
    assert_eq!(calls.sorted(), vec!["pending_for_account"]);
}

#[tokio::test]
async fn scrubber_double_runs_one_step_run_to_done() {
    // Arrange
    let calls = Calls::default();
    let scrubber: Arc<dyn Scrubber> = Arc::new(Double(calls.clone()));

    // Act
    let run = scrubber
        .start_run(ScrubMode::Due, Some(ByteRate(1 << 20)))
        .await
        .expect("start");
    let step = scrubber.run_step(&run).await.expect("step");
    scrubber.finish_run(&run).await.expect("finish");
    let status = scrubber.status().await.expect("status");

    // Assert
    assert!(step.done);
    assert_eq!(status.unavailable, 1);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["start_run", "run_step", "finish_run", "status"])
    );
}

#[tokio::test]
async fn inventory_keyring_and_marker_doubles_record_their_calls() {
    // Arrange
    let calls = Calls::default();
    let inventory: Arc<dyn BlobInventory> = Arc::new(Double(calls.clone()));
    let keyring: Arc<dyn MasterKeyring> = Arc::new(Double(calls.clone()));
    let marker: Arc<dyn EpochMarker> = Arc::new(Double(calls.clone()));

    // Act
    let missing = inventory.find_missing(100).await.expect("missing");
    let ring = keyring.load_and_verify().await.expect("ring");
    let short_key = keyring.init_key_check(&MasterKey(vec![0; 8])).await;
    let rewrapped = keyring.rewrap_batch(1, 2, 50).await.expect("rewrap");
    marker.write(7).await.expect("write");
    let read = marker.read().await.expect("read");

    // Assert
    assert_eq!(missing, vec![id()]);
    assert_eq!(ring.current_key_id, 2);
    assert!(matches!(short_key, Err(KeyError::WrongLength)));
    assert_eq!(rewrapped, 50);
    assert_eq!(read, None);
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "find_missing",
            "load_and_verify",
            "init_key_check",
            "rewrap_batch",
            "write",
            "read"
        ])
    );
}
