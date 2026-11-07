use std::sync::Arc;
use std::time::Duration;

use super::support::{Rig, wait_until};

#[tokio::test(flavor = "multi_thread")]
async fn writers_arriving_during_a_flush_join_the_next_group() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    let (first_id, first_file) = rig.scratch.file();
    let first = rig.write_and_register(first_id, &first_file, 0);
    let first_waiter = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.await_durable(first.token).await })
    };
    wait_until(|| rig.hook.flush_count() == 1).await;

    let (late_id, late_file) = rig.scratch.file();
    let (later_id, later_file) = rig.scratch.file();
    let late = rig.write_and_register(late_id, &late_file, 0);
    let later = rig.write_and_register(later_id, &later_file, 0);
    assert_eq!(late.token, later.token, "late arrivals share one group");
    assert_ne!(late.token, first.token, "and it is not the flushing one");
    let late_waiters: Vec<_> = [late.token, later.token]
        .into_iter()
        .map(|token| {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move { rig.barrier.await_durable(token).await })
        })
        .collect();

    rig.hook.release();
    first_waiter
        .await
        .expect("task finishes")
        .expect("first group syncs");
    for waiter in late_waiters {
        waiter
            .await
            .expect("task finishes")
            .expect("second group syncs");
    }

    let flush_of = |id| {
        rig.hook
            .records()
            .iter()
            .find(|record| record.blob_id == id)
            .map(|record| record.flush)
    };
    assert_eq!(rig.hook.flush_count(), 2);
    assert_eq!(flush_of(first_id), Some(1));
    assert_eq!(flush_of(late_id), Some(2));
    assert_eq!(flush_of(later_id), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_leader_does_not_strand_the_other_waiters_of_its_group() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    let (id_a, file_a) = rig.scratch.file();
    let (id_b, file_b) = rig.scratch.file();
    let leader = rig.write_and_register(id_a, &file_a, 0);
    let follower = rig.write_and_register(id_b, &file_b, 0);
    assert_eq!(leader.token, follower.token);

    let cancelled = tokio::time::timeout(
        Duration::from_millis(50),
        rig.barrier.await_durable(leader.token),
    )
    .await;
    assert!(cancelled.is_err(), "the leader was still waiting");
    let waiter = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.await_durable(follower.token).await })
    };
    rig.hook.release();

    waiter
        .await
        .expect("task finishes")
        .expect("the follower is released by the flush the leader started");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_blob_registered_twice_in_a_group_is_synced_once() {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    let (id, file) = rig.scratch.file();
    let first = rig.write_and_register(id, &file, 0);
    let second = rig.write_and_register(id, &file, 4096);
    let waiter = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.await_durable(first.token).await })
    };
    wait_until(|| rig.hook.flush_count() == 1).await;
    rig.hook.release();
    waiter.await.expect("task finishes").expect("sync succeeds");
    rig.barrier
        .await_durable(second.token)
        .await
        .expect("the second registration shares the outcome");

    assert_eq!(rig.hook.records().len(), 1);
}
