use std::sync::Arc;
use std::time::{Duration, Instant};

use super::support::{Rig, wait_until};

const BLOCKED: Duration = Duration::from_millis(300);

#[tokio::test(flavor = "multi_thread")]
async fn no_await_durable_returns_before_the_blocked_sync_ends() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.delay_flushes(BLOCKED);
    let started = Instant::now();

    let waiters: Vec<_> = (0..4)
        .map(|lane| {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move {
                let (id, file) = rig.scratch.file();
                let written = rig.write_and_register(id, &file, lane * 4096);
                rig.barrier
                    .await_durable(written.token)
                    .await
                    .map(|()| started.elapsed())
            })
        })
        .collect();

    for waiter in waiters {
        let elapsed = waiter.await.expect("task finishes").expect("sync succeeds");
        assert!(elapsed >= BLOCKED, "acknowledged after {elapsed:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_sync_keeps_every_waiter_pending_until_it_is_released() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    let (id, file) = rig.scratch.file();
    let written = rig.write_and_register(id, &file, 0);
    let waiter = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.await_durable(written.token).await })
    };

    wait_until(|| rig.hook.flush_count() == 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "released while the sync was held");
    assert!(rig.hook.records().is_empty());

    rig.hook.release();
    waiter.await.expect("task finishes").expect("sync succeeds");
    assert_eq!(rig.hook.records().len(), 1);
}
