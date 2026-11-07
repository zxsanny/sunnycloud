use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, DurableToken};

use super::support::{Rig, wait_until};

type Waiter = tokio::task::JoinHandle<Result<(), BlobError>>;

fn register(rig: &Rig, offset: u64) -> (uuid::Uuid, DurableToken) {
    let (id, file) = rig.scratch.file();
    (id, rig.write_and_register(id, &file, offset).token)
}

fn wait(rig: &Arc<Rig>, token: DurableToken) -> Waiter {
    let rig = Arc::clone(rig);
    tokio::spawn(async move { rig.barrier.await_durable(token).await })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_sync_gives_io_to_every_waiter_of_its_group_and_the_next_group_succeeds() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    rig.hook.fail_next_flushes(1);
    let failed: Vec<_> = (0..3).map(|n| register(&rig, n * 4096)).collect();
    let failed: Vec<_> = failed
        .into_iter()
        .map(|(id, token)| (id, wait(&rig, token)))
        .collect();
    wait_until(|| rig.hook.flush_count() == 1).await;
    let next: Vec<_> = (0..2).map(|n| register(&rig, n * 4096)).collect();
    let next: Vec<_> = next
        .into_iter()
        .map(|(id, token)| (id, wait(&rig, token)))
        .collect();

    rig.hook.release();

    let first_blob = failed[0].0;
    for (_, waiter) in failed {
        match waiter.await.expect("task finishes") {
            Err(BlobError::Io(fault)) => {
                let text = fault.to_string();
                assert!(text.contains(&first_blob.to_string()), "ids only: {text}");
                assert!(!text.contains("sc-barrier"), "no path in the error: {text}");
            }
            other => panic!("expected Io, got {other:?}"),
        }
    }
    for (_, waiter) in next {
        waiter
            .await
            .expect("task finishes")
            .expect("the next group syncs once the fault clears");
    }
    assert_eq!(rig.hook.flush_count(), 2);
    assert_eq!(
        rig.hook.records().len(),
        2,
        "the failed group synced nothing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn consecutive_failures_each_fail_their_own_group_and_recovery_follows() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.fail_next_flushes(2);

    for _ in 0..2 {
        let (_, token) = register(&rig, 0);
        assert!(matches!(
            wait(&rig, token).await.expect("task finishes"),
            Err(BlobError::Io(_))
        ));
    }
    let (_, token) = register(&rig, 0);
    wait(&rig, token)
        .await
        .expect("task finishes")
        .expect("a group after the fault clears is durable");
}
