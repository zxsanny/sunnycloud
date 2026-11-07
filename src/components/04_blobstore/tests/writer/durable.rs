//! AC-8 and the retry rule: a block is acknowledged only after an `fdatasync` that started after its write, and
//! after a failed sync the block is written again, never just awaited again.

use std::time::{Duration, Instant};

use contracts::{BlobError, BlobWriter};

use crate::support::{MIB, Rig, block_bytes, created, decrypt};

#[tokio::test(flavor = "multi_thread")]
async fn await_durable_does_not_return_before_a_blocked_sync_ends() {
    // Arrange
    let rig = Rig::new().await;
    let (handle, source) = created(&rig, 3 * MIB, true).await;
    rig.hook.delay_flushes(Duration::from_millis(300));
    let token = rig
        .writer
        .write_block(&handle, 0, block_bytes(&source, 0))
        .await
        .expect("block is written");
    let written = rig.hook.tick();

    // Act
    let started = Instant::now();
    rig.writer.await_durable(token).await.expect("durable");

    // Assert
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
    let records = rig.hook.records();
    assert_eq!(records.len(), 1);
    assert!(
        records[0].stamp > written,
        "the sync started after the write completed"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_failed_sync_the_block_is_written_again_and_a_second_await_of_the_old_token_is_io()
{
    // Arrange
    let rig = Rig::new().await;
    let (handle, source) = created(&rig, 3 * MIB, false).await;
    rig.hook.fail_next_flushes(1);
    let token = rig
        .writer
        .write_block(&handle, 1, block_bytes(&source, 1))
        .await
        .expect("block is written");

    // Act
    let failed = rig.writer.await_durable(token).await;
    let retry_alone = rig.writer.await_durable(token).await;
    let rewrite = rig
        .writer
        .write_block(&handle, 1, block_bytes(&source, 1))
        .await
        .expect("block is rewritten");
    let rewritten = rig.hook.tick();
    let acknowledged = rig.writer.await_durable(rewrite).await;

    // Assert
    assert!(matches!(failed, Err(BlobError::Io(_))), "{failed:?}");
    assert!(
        matches!(retry_alone, Err(BlobError::Io(_))),
        "{retry_alone:?}"
    );
    acknowledged.expect("the rewritten block is durable");
    let records = rig.hook.records();
    assert_eq!(records.len(), 1, "the failed group synced nothing");
    assert!(
        records[0].stamp > rewritten,
        "the acknowledging sync started after the rewrite"
    );
    let token = rig
        .writer
        .write_block(&handle, 0, block_bytes(&source, 0))
        .await
        .expect("block");
    rig.writer.await_durable(token).await.expect("durable");
    assert_eq!(decrypt(&rig, &handle.plan, 3 * MIB), *source);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_blocks_of_many_files_are_all_durable_after_their_last_write() {
    // Arrange
    let rig = Rig::new().await;
    let mut blobs = Vec::new();
    for _ in 0..10 {
        blobs.push(created(&rig, 5 * MIB, false).await);
    }

    // Act
    let mut tasks = Vec::new();
    for (handle, source) in &blobs {
        for index in 0..3 {
            let (writer, handle, source) = (rig.writer.clone(), handle.clone(), source.clone());
            tasks.push(tokio::spawn(async move {
                let token = writer
                    .write_block(&handle, index, block_bytes(&source, index))
                    .await
                    .expect("block");
                writer.await_durable(token).await.expect("durable");
            }));
        }
    }
    for task in tasks {
        task.await.expect("task");
    }

    // Assert
    for (handle, source) in &blobs {
        assert_eq!(decrypt(&rig, &handle.plan, 5 * MIB), **source);
        assert!(
            rig.hook
                .records()
                .iter()
                .any(|r| r.blob_id == handle.plan.blob_id)
        );
    }
    rig.finish().await;
}
