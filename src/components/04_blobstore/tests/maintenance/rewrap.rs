//! AC-1: rotation by rewrap. The data keys move from key 1 to key 2 in short batches; the files are never touched.

use contracts::KeyError;
use uuid::Uuid;

use crate::fixture::{Rotated, reference, snapshot};
use crate::support::Rig;

const BLOBS: usize = 450;
const LEN: usize = 1024;

async fn under_key(rig: &Rig, key_id: i16) -> i64 {
    rig.count(&format!(
        "SELECT count(*) FROM blobs WHERE master_key_id = {key_id}"
    ))
    .await
}

async fn wraps(rig: &Rig, ids: &[Uuid]) -> Vec<Vec<u8>> {
    let mut wraps = Vec::new();
    for id in ids {
        wraps.push(reference(rig, *id).await.wrapped_key);
    }
    wraps
}

async fn store_all(rig: &Rig, count: usize) -> Vec<Uuid> {
    let mut ids = Vec::with_capacity(count);
    for n in 0..count {
        ids.push(rig.store(LEN, (n % 251) as u8).await.blob_id);
    }
    ids
}

#[tokio::test(flavor = "multi_thread")]
async fn four_hundred_fifty_blobs_move_to_key_two_with_an_interrupted_run_and_the_files_untouched()
{
    // Arrange
    let rig = Rig::new().await;
    let ids = store_all(&rig, BLOBS).await;
    let files_before = snapshot(&rig, &ids);
    let first = Rotated::start(&rig).await;

    // Act
    let mut batches = Vec::new();
    for _ in 0..2 {
        batches.push(first.keyring.rewrap_batch(1, 2, 100).await.expect("batch"));
    }
    let (mid_key_one, mid_key_two) = (under_key(&rig, 1).await, under_key(&rig, 2).await);
    let mut mid_reads_match = true;
    for (n, id) in ids.iter().enumerate() {
        mid_reads_match &= first.head(&rig, *id).await == vec![(n % 251) as u8; LEN];
    }
    drop(first);
    let resumed = Rotated::start(&rig).await;
    loop {
        let handled = resumed
            .keyring
            .rewrap_batch(1, 2, 100)
            .await
            .expect("batch");
        batches.push(handled);
        if handled == 0 {
            break;
        }
    }
    let files_after = snapshot(&rig, &ids);
    let mut reads_match = true;
    for (n, id) in ids.iter().enumerate() {
        reads_match &= resumed.head(&rig, *id).await == vec![(n % 251) as u8; LEN];
    }
    let fresh = resumed.store(&rig, LEN, 9).await;

    // Assert
    assert_eq!(batches, vec![100, 100, 100, 100, 50, 0]);
    assert_eq!((mid_key_one, mid_key_two), (250, 200));
    assert!(mid_reads_match, "reads work between the batches");
    assert_eq!(under_key(&rig, 1).await, 0);
    assert_eq!(under_key(&rig, 2).await, 451);
    assert!(files_before == files_after, "no blob file changed");
    assert!(reads_match, "reads work after the rotation");
    assert_eq!(fresh.master_key_id, 2, "new blobs use the current key");
    drop(resumed);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_run_finds_no_row_and_leaves_every_wrap_as_it_was() {
    // Arrange
    let rig = Rig::new().await;
    let ids = store_all(&rig, 5).await;
    let rotated = Rotated::start(&rig).await;
    let first = rotated.keyring.rewrap_batch(1, 2, 10).await.expect("run");
    let after_first = wraps(&rig, &ids).await;

    // Act
    let second = rotated.keyring.rewrap_batch(1, 2, 10).await.expect("rerun");
    let after_second = wraps(&rig, &ids).await;

    // Assert
    assert_eq!((first, second), (5, 0));
    assert!(
        after_first == after_second,
        "a rewrapped row is not rewrapped again"
    );
    drop(rotated);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_runs_at_once_leave_every_row_on_key_two_and_each_counts_the_rows_it_handled() {
    // Arrange
    let rig = Rig::new().await;
    let ids = store_all(&rig, 20).await;
    let one = Rotated::start(&rig).await;
    let other = Rotated::start(&rig).await;

    // Act
    let (a, b) = tokio::join!(
        one.keyring.rewrap_batch(1, 2, 20),
        other.keyring.rewrap_batch(1, 2, 20)
    );
    let (a, b) = (a.expect("first run"), b.expect("second run"));
    let rest = one.keyring.rewrap_batch(1, 2, 20).await.expect("rest");
    let mut reads_match = true;
    for (n, id) in ids.iter().enumerate() {
        reads_match &= other.head(&rig, *id).await == vec![(n % 251) as u8; LEN];
    }

    // Assert
    assert!(a <= 20 && b <= 20 && a + b >= 20, "handled {a} and {b}");
    assert_eq!(rest, 0);
    assert_eq!(under_key(&rig, 2).await, 20);
    assert!(reads_match);
    drop((one, other));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_in_every_state_are_rewrapped() {
    // Arrange
    let rig = Rig::new().await;
    rig.store(LEN, 1).await;
    let uploading = rig.half_uploaded().await;
    let rotated = Rotated::start(&rig).await;

    // Act
    let handled = rotated.keyring.rewrap_batch(1, 2, 10).await.expect("run");

    // Assert
    assert_eq!(handled, 2);
    assert_eq!(under_key(&rig, 1).await, 0);
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM blobs WHERE id = '{uploading}' AND master_key_id = 2"
        ))
        .await,
        1
    );
    drop(rotated);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_with_an_unopenable_wrap_fails_and_changes_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let ids = store_all(&rig, 3).await;
    rig.db
        .admin_exec(&format!(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             VALUES ('{}', '{}', 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
            Uuid::new_v4(),
            rig.account
        ))
        .await;
    let before = wraps(&rig, &ids).await;
    let rotated = Rotated::start(&rig).await;

    // Act
    let result = rotated.keyring.rewrap_batch(1, 2, 100).await;
    let after = wraps(&rig, &ids).await;

    // Assert
    assert!(matches!(result, Err(KeyError::Unexpected(_))), "{result:?}");
    assert!(before == after);
    assert_eq!(under_key(&rig, 1).await, 4);
    drop(rotated);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ids_the_ring_does_not_hold_or_that_are_equal_are_refused_and_zero_rows_is_zero() {
    // Arrange
    let rig = Rig::new().await;
    store_all(&rig, 2).await;
    let rotated = Rotated::start(&rig).await;

    // Act
    let unknown_target = rotated.keyring.rewrap_batch(1, 9, 10).await;
    let unknown_source = rotated.keyring.rewrap_batch(9, 2, 10).await;
    let same = rotated.keyring.rewrap_batch(1, 1, 10).await;
    let none = rotated.keyring.rewrap_batch(1, 2, 0).await;

    // Assert
    assert!(matches!(unknown_target, Err(KeyError::Unexpected(_))));
    assert!(matches!(unknown_source, Err(KeyError::Unexpected(_))));
    assert!(matches!(same, Err(KeyError::Unexpected(_))));
    assert_eq!(none.expect("n = 0"), 0);
    assert_eq!(under_key(&rig, 1).await, 2);
    drop(rotated);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_port_rewraps_through_the_trait_object() {
    // Arrange
    let rig = Rig::new().await;
    store_all(&rig, 3).await;
    let rotated = Rotated::start(&rig).await;
    let port: &dyn contracts::MasterKeyring = rotated.keyring.as_ref();

    // Act
    let handled = port.rewrap_batch(1, 2, 2).await.expect("batch");
    let rest = port.rewrap_batch(1, 2, 2).await.expect("batch");
    let done = port.rewrap_batch(1, 2, 2).await.expect("batch");

    // Assert
    assert_eq!((handled, rest, done), (2, 1, 0));
    drop(rotated);
    rig.finish().await;
}
