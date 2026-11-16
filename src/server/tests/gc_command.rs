//! AZ-3796 AC-7: `admin gc-run` on the real sweeper of the composition root over an empty queue.

mod root_support;
mod support;

use std::sync::Arc;

use sc_clock::{Clock, SystemClock};
use sunnycloud_blobstore::GcSweeper;
use support::Harness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gc_run_prints_the_counts_of_one_cycle_and_exits_0_when_nothing_is_due() {
    // Arrange
    let root = root_support::Root::new().await;
    let services = root.services.clone();
    let harness = Harness::new()
        .real_handler()
        .service::<GcSweeper, _, _>(move |_| {
            let services = services.clone();
            async move { services.get::<GcSweeper>().await }
        })
        .service::<dyn Clock, _, _>(|_| async {
            Ok(Arc::new(SystemClock::new()) as Arc<dyn Clock>)
        });

    // Act
    let result = tokio::task::spawn_blocking(move || harness.run(&["admin", "gc-run"]))
        .await
        .expect("the command thread");

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    for line in ["removed: 0", "remaining_due: 0", "budget_spent: false"] {
        assert!(result.stdout.contains(line), "{line}: {}", result.stdout);
    }
    root.finish().await;
}
