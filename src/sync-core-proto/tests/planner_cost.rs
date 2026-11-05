//! AC-5 (PT-13-3): planner cost for a 50 GiB file, 25,600 blocks of 2 MiB.

use std::time::{Duration, Instant};

use sync_core_proto::{Action, BlockResult, RetryPolicy, UploadPlanner};

const BLOCKS: u32 = 25_600;
const MEAN_TARGET: Duration = Duration::from_micros(50);
const MEAN_FAILURE: Duration = Duration::from_millis(1);
const STATE_TARGET: usize = 4 * 1024 * 1024;
const STATE_FAILURE: usize = 16 * 1024 * 1024;

#[test]
fn a_50_gib_plan_costs_at_most_50_microseconds_per_call_and_4_mb() {
    let policy = RetryPolicy {
        base: Duration::from_millis(500),
        max: Duration::from_secs(30),
        max_attempts_per_block: 8,
    };
    let all: Vec<u32> = (0..BLOCKS).collect();
    let mut planner = UploadPlanner::new(BLOCKS, &all, 4, policy);
    let state = planner.approx_state_bytes();
    let mut calls = 0u32;
    let mut spent = Duration::ZERO;
    let mut sent = 0u32;
    let mut committed = false;

    while !committed {
        let started = Instant::now();
        let actions = planner.next_actions();
        spent += started.elapsed();
        calls += 1;
        assert!(!actions.is_empty(), "stalled after {sent} blocks");
        for action in actions {
            match action {
                Action::SendBlock { i } => {
                    sent += 1;
                    planner.on_result(i, BlockResult::Acked);
                }
                Action::Commit => committed = true,
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    let mean = spent / calls;
    assert_eq!(sent, BLOCKS);
    assert!(mean < MEAN_FAILURE, "mean {mean:?} above the failure bound");
    assert!(mean <= MEAN_TARGET, "mean {mean:?} above the target");
    assert!(
        state < STATE_FAILURE,
        "state {state} bytes above the failure bound"
    );
    assert!(
        state <= STATE_TARGET,
        "state {state} bytes above the target"
    );
    assert!(planner.approx_state_bytes() <= STATE_TARGET);
}
