use std::fs::File;
use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, DurableToken};
use sunnycloud_blobstore::SyncBarrier;
use uuid::Uuid;

use super::support::{Scratch, write_at};

#[tokio::test(flavor = "multi_thread")]
async fn the_production_barrier_syncs_a_regular_file() {
    let scratch = Scratch::new();
    let barrier = SyncBarrier::new(Duration::ZERO);
    let (id, file) = scratch.file();
    write_at(&file, 0);

    let token = barrier.register(id, file);

    barrier
        .await_durable(token)
        .await
        .expect("fdatasync succeeds");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_production_barrier_returns_the_failure_of_the_real_system_call() {
    let barrier = SyncBarrier::new(Duration::ZERO);
    let device = Arc::new(File::open("/dev/null").expect("the null device opens"));
    let id = Uuid::new_v4();

    let token = barrier.register(id, device);

    let error = barrier
        .await_durable(token)
        .await
        .expect_err("fdatasync on a device that cannot be synced fails");
    match error {
        BlobError::Io(fault) => assert!(fault.to_string().contains(&id.to_string())),
        other => panic!("expected Io, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_or_already_awaited_token_is_io_never_ok() {
    let scratch = Scratch::new();
    let barrier = SyncBarrier::new(Duration::ZERO);
    let (id, file) = scratch.file();
    let token = barrier.register(id, file);
    barrier.await_durable(token).await.expect("first await");

    let again = barrier.await_durable(token).await;
    let unknown = barrier.await_durable(DurableToken(u64::MAX)).await;

    assert!(matches!(again, Err(BlobError::Io(_))));
    assert!(matches!(unknown, Err(BlobError::Io(_))));
}
