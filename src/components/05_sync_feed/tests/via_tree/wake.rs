//! AC-2 / AT-05-1: device B parked at head with `wait = 300 ms` on the system clock wakes when device A publishes
//! a file through 02. The latency is measured server side, from the notify `publish` sends after COMMIT to the
//! return of B's read (AC-5 share: 50 ms).

use std::sync::Arc;
use std::time::Duration;

use sc_clock::SystemClock;

use crate::support::{Rig, new_file, published};

const WAIT: Duration = Duration::from_millis(300);
const WAKE_BOUND: Duration = Duration::from_millis(50);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_b_parked_at_head_gets_the_new_node_within_fifty_ms_of_device_a_publishing() {
    // Arrange
    let rig = Rig::new(Arc::new(SystemClock::new()), 500).await;
    let blobs = rig.blobs(2).await;
    rig.publish("device-a", new_file(rig.root, "earlier.txt", blobs[0]))
        .await
        .expect("an earlier publish");
    let head = rig.reader.head(rig.account).await.expect("head");
    let held = rig.hold(head, WAIT);
    rig.clock.sleeps_reach(1).await;

    // Act
    let result = rig
        .publish("device-a", new_file(rig.root, "new.txt", blobs[1]))
        .await
        .expect("publish");
    let (page, returned) = held.await.expect("hold task");

    // Assert
    let (node, revision) = published(&result);
    let (seq, notified) = *rig.notifies().last().expect("a notify");
    let latency = returned.saturating_duration_since(notified);
    println!("AT-05-1: wake latency {latency:?} (notify after COMMIT -> B's read returned)");
    assert_eq!(seq, head.seq + 1, "the counter advanced by one");
    assert_eq!(rig.counter().await, head.seq + 1);
    let page = page.expect("page");
    assert_eq!(
        page.items
            .iter()
            .map(|item| (item.node_id, item.revision_id, item.seq))
            .collect::<Vec<_>>(),
        [(node, Some(revision), seq)],
        "B's page holds the new node at the new seq"
    );
    assert_eq!(page.next_cursor.seq, seq);
    assert!(latency <= WAKE_BOUND, "B woke {latency:?} after the notify");
    rig.finish().await;
}
