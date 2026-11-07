use std::sync::Arc;
use std::time::{Duration, Instant};

use sunnycloud_blobstore::SyncBarrier;

use super::support::{Rig, Scratch, write_at};

#[tokio::test(flavor = "multi_thread")]
async fn an_uncontended_write_is_acknowledged_within_twenty_milliseconds() {
    let scratch = Scratch::new();
    let barrier = SyncBarrier::new(Duration::ZERO);
    let (id, file) = scratch.file();

    let started = Instant::now();
    write_at(&file, 0);
    let token = barrier.register(id, Arc::clone(&file));
    barrier
        .await_durable(token)
        .await
        .expect("the real sync succeeds");

    assert!(
        started.elapsed() < Duration::from_millis(20),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_coalesce_delay_gathers_writers_into_one_flush_that_starts_after_it() {
    let delay = Duration::from_millis(150);
    let rig = Arc::new(Rig::new(delay));
    let started = Instant::now();

    let waiters: Vec<_> = (0..3u64)
        .map(|lane| {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(lane * 30)).await;
                let (id, file) = rig.scratch.file();
                let written = rig.write_and_register(id, &file, 0);
                rig.barrier.await_durable(written.token).await?;
                Ok::<_, contracts::BlobError>(started.elapsed())
            })
        })
        .collect();

    for waiter in waiters {
        let elapsed = waiter.await.expect("task finishes").expect("sync succeeds");
        assert!(elapsed >= delay, "acknowledged after {elapsed:?}");
    }
    assert_eq!(rig.hook.flush_count(), 1);
}
