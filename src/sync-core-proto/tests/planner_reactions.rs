//! AC-2 and AC-3 (IT-13-2) plus the edge rules of the planner: lanes, delays, resume diff, terminals.

use std::time::Duration;

use sync_core_proto::{Action, BlockResult, CommitResult, RetryPolicy, UploadPlanner};

const POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_millis(100),
    max: Duration::from_secs(30),
    max_attempts_per_block: 8,
};

fn all(n: u32) -> Vec<u32> {
    (0..n).collect()
}

fn send(i: u32) -> Action {
    Action::SendBlock { i }
}

fn wait(ms: u64) -> Action {
    Action::Wait { ms }
}

fn conflict(secs: Option<u64>) -> BlockResult {
    BlockResult::BlockConflict {
        retry_after: secs.map(Duration::from_secs),
    }
}

fn transient(secs: Option<u64>) -> BlockResult {
    BlockResult::Transient {
        retry_after: secs.map(Duration::from_secs),
    }
}

/// A one-block planner whose only block is in flight.
fn single_in_flight() -> UploadPlanner {
    let mut planner = UploadPlanner::new(1, &[0], 4, POLICY);
    assert_eq!(planner.next_actions(), vec![send(0)]);
    planner
}

/// Sends everything the planner asks for, acking each block, until `Commit`; returns the blocks sent.
fn ack_until_commit(planner: &mut UploadPlanner) -> Vec<u32> {
    let mut sent = Vec::new();
    loop {
        let actions = planner.next_actions();
        assert!(!actions.is_empty(), "stalled with nothing in flight");
        for action in actions {
            match action {
                Action::SendBlock { i } => {
                    sent.push(i);
                    planner.on_result(i, BlockResult::Acked);
                }
                Action::Commit => return sent,
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}

#[test]
fn a_session_listing_limits_sending_to_the_needed_blocks() {
    let mut planner = UploadPlanner::new(100, &all(100), 4, POLICY);

    planner.on_session(&[5, 6, 9]);

    assert_eq!(
        planner.next_actions(),
        vec![send(5), send(6), send(9)],
        "only the listed blocks"
    );
    for i in [5, 6, 9] {
        planner.on_result(i, BlockResult::Acked);
    }
    assert_eq!(planner.next_actions(), vec![Action::Commit]);
}

#[test]
fn construction_with_a_resume_list_sends_only_that_list() {
    let mut planner = UploadPlanner::new(100, &[9, 5, 6, 5], 4, POLICY);

    assert_eq!(ack_until_commit(&mut planner), vec![5, 6, 9]);
}

#[test]
fn a_hash_rejection_sends_the_block_again() {
    let mut planner = single_in_flight();

    planner.on_result(0, BlockResult::HashRejected);

    assert_eq!(planner.next_actions(), vec![send(0)]);
}

#[test]
fn a_conflict_on_a_still_needed_block_waits_rereads_then_resends() {
    let mut planner = single_in_flight();

    planner.on_result(0, conflict(Some(2)));

    assert_eq!(
        planner.next_actions(),
        vec![wait(2000), Action::RereadSession]
    );
    assert_eq!(
        planner.next_actions(),
        Vec::<Action>::new(),
        "nothing is sent until the listing arrives"
    );
    planner.on_session(&[0]);
    assert_eq!(planner.next_actions(), vec![send(0)]);
}

#[test]
fn a_conflict_on_a_block_no_longer_needed_and_sent_by_this_run_counts_as_acked() {
    let mut planner = single_in_flight();

    planner.on_result(0, conflict(Some(2)));
    planner.next_actions();
    planner.on_session(&[]);

    assert_eq!(planner.next_actions(), vec![Action::Commit]);
}

#[test]
fn a_conflict_on_a_block_not_sent_by_this_run_restarts_the_session() {
    let mut planner = UploadPlanner::new(100, &all(100), 4, POLICY);

    planner.on_result(50, conflict(Some(1)));
    assert_eq!(
        planner.next_actions(),
        vec![wait(1000), Action::RereadSession]
    );
    let rest: Vec<u32> = all(100).into_iter().filter(|i| *i != 50).collect();
    planner.on_session(&rest);

    assert_eq!(planner.next_actions(), vec![Action::RestartSession]);
}

#[test]
fn a_conflict_without_retry_after_waits_the_backoff() {
    let mut planner = single_in_flight();

    planner.on_result(0, conflict(None));

    assert_eq!(
        planner.next_actions(),
        vec![wait(100), Action::RereadSession]
    );
}

#[test]
fn the_longest_conflict_wait_covers_all_conflicts_of_one_reread() {
    let mut planner = UploadPlanner::new(2, &[0, 1], 4, POLICY);
    assert_eq!(planner.next_actions(), vec![send(0), send(1)]);

    planner.on_result(0, conflict(Some(1)));
    planner.on_result(1, conflict(Some(3)));

    assert_eq!(
        planner.next_actions(),
        vec![wait(3000), Action::RereadSession]
    );
    planner.on_session(&[1]);
    assert_eq!(planner.next_actions(), vec![send(1)]);
}

#[test]
fn a_commit_answer_with_needed_blocks_sends_them_then_commits_again() {
    let mut planner = UploadPlanner::new(10, &all(10), 4, POLICY);
    ack_until_commit(&mut planner);

    planner.on_commit(CommitResult::NeedBlocks { list: vec![7] });

    assert_eq!(planner.next_actions(), vec![send(7)]);
    planner.on_result(7, BlockResult::Acked);
    assert_eq!(planner.next_actions(), vec![Action::Commit]);
    planner.on_commit(CommitResult::Ok);
    assert_eq!(planner.next_actions(), vec![Action::Done]);
}

#[test]
fn a_blocklist_mismatch_restarts_the_session() {
    let mut planner = UploadPlanner::new(2, &all(2), 4, POLICY);
    ack_until_commit(&mut planner);

    planner.on_commit(CommitResult::BlocklistMismatch);

    assert_eq!(planner.next_actions(), vec![Action::RestartSession]);
}

#[test]
fn session_gone_restarts_on_a_block_result_and_on_a_commit_answer() {
    let mut on_block = single_in_flight();
    on_block.on_result(0, BlockResult::SessionGone);
    let mut on_commit = UploadPlanner::new(1, &[0], 4, POLICY);
    ack_until_commit(&mut on_commit);
    on_commit.on_commit(CommitResult::SessionGone);

    assert_eq!(on_block.next_actions(), vec![Action::RestartSession]);
    assert_eq!(on_commit.next_actions(), vec![Action::RestartSession]);
}

#[test]
fn eight_transient_results_end_in_give_up_instead_of_a_ninth_send() {
    let mut planner = UploadPlanner::new(1, &[0], 4, POLICY);
    let mut sends = 0;

    for attempt in 1..=8 {
        let actions = planner.next_actions();
        assert_eq!(actions.last(), Some(&send(0)), "attempt {attempt}");
        sends += 1;
        planner.on_result(0, transient(None));
    }

    assert_eq!(sends, 8);
    assert_eq!(planner.next_actions(), vec![Action::GiveUp { i: 0 }]);
    assert_eq!(
        planner.next_actions(),
        vec![Action::GiveUp { i: 0 }],
        "the terminal action is sticky"
    );
}

#[test]
fn transient_retries_wait_the_backoff_or_the_server_hint() {
    let mut planner = UploadPlanner::new(1, &[0], 4, POLICY);

    assert_eq!(planner.next_actions(), vec![send(0)]);
    planner.on_result(0, transient(None));
    assert_eq!(planner.next_actions(), vec![wait(100), send(0)]);
    planner.on_result(0, transient(None));
    assert_eq!(planner.next_actions(), vec![wait(200), send(0)]);
    planner.on_result(0, transient(Some(7)));
    assert_eq!(
        planner.next_actions(),
        vec![wait(7000), send(0)],
        "retry_after is taken as given"
    );
}

#[test]
fn the_backoff_doubles_up_to_max_and_never_overflows() {
    let policy = RetryPolicy {
        base: Duration::from_millis(100),
        max: Duration::from_millis(300),
        max_attempts_per_block: 8,
    };

    let delays: Vec<Duration> = [0, 1, 2, 3, 4, u32::MAX]
        .iter()
        .map(|attempts| policy.backoff(*attempts))
        .collect();

    let ms = |value| Duration::from_millis(value);
    assert_eq!(
        delays,
        vec![ms(100), ms(100), ms(200), ms(300), ms(300), ms(300)]
    );
}

#[test]
fn repeated_hash_rejections_also_end_in_give_up() {
    let mut planner = UploadPlanner::new(1, &[0], 4, POLICY);

    for _ in 0..8 {
        assert_eq!(planner.next_actions(), vec![send(0)]);
        planner.on_result(0, BlockResult::HashRejected);
    }

    assert_eq!(planner.next_actions(), vec![Action::GiveUp { i: 0 }]);
}

#[test]
fn a_resend_after_a_conflict_counts_against_the_attempts() {
    let policy = RetryPolicy {
        max_attempts_per_block: 2,
        ..POLICY
    };
    let mut planner = UploadPlanner::new(1, &[0], 4, policy);

    for _ in 0..2 {
        assert_eq!(planner.next_actions().last(), Some(&send(0)));
        planner.on_result(0, conflict(Some(1)));
        planner.next_actions();
        planner.on_session(&[0]);
    }

    assert_eq!(planner.next_actions(), vec![Action::GiveUp { i: 0 }]);
}

#[test]
fn zero_max_attempts_counts_as_one() {
    let policy = RetryPolicy {
        max_attempts_per_block: 0,
        ..POLICY
    };
    let mut planner = UploadPlanner::new(1, &[0], 4, policy);

    assert_eq!(planner.next_actions(), vec![send(0)]);
    planner.on_result(0, transient(None));
    assert_eq!(planner.next_actions(), vec![Action::GiveUp { i: 0 }]);
}

#[test]
fn lanes_are_the_lower_of_four_and_the_server_limit() {
    let first = |lanes_max| UploadPlanner::new(10, &all(10), lanes_max, POLICY).next_actions();

    assert_eq!(first(0).len(), 1, "no lane limit still allows one lane");
    assert_eq!(first(1).len(), 1);
    assert_eq!(first(2).len(), 2);
    assert_eq!(first(4).len(), 4);
    assert_eq!(first(8).len(), 4);
}

#[test]
fn a_free_lane_is_refilled_one_block_per_result() {
    let mut planner = UploadPlanner::new(10, &all(10), 8, POLICY);
    assert_eq!(
        planner.next_actions(),
        vec![send(0), send(1), send(2), send(3)]
    );
    assert!(planner.next_actions().is_empty(), "lanes are full");

    planner.on_result(1, BlockResult::Acked);

    assert_eq!(planner.next_actions(), vec![send(4)]);
}

#[test]
fn a_listing_keeps_blocks_in_flight_so_the_lane_count_holds() {
    let mut planner = UploadPlanner::new(8, &all(8), 4, POLICY);
    assert_eq!(planner.next_actions().len(), 4);

    planner.on_session(&[2, 3, 4, 5]);

    assert!(planner.next_actions().is_empty(), "blocks 0..3 still fly");
    planner.on_result(0, transient(None));
    assert_eq!(
        planner.next_actions(),
        vec![send(4)],
        "block 0 is no longer needed, so it is not resent"
    );
    planner.on_result(1, BlockResult::Acked);
    assert_eq!(planner.next_actions(), vec![send(5)]);
    planner.on_result(2, BlockResult::Acked);
    planner.on_result(3, BlockResult::Acked);
    planner.on_result(4, BlockResult::Acked);
    planner.on_result(5, BlockResult::Acked);
    assert_eq!(planner.next_actions(), vec![Action::Commit]);
}

#[test]
fn a_block_listed_again_after_its_ack_is_sent_again() {
    let mut planner = UploadPlanner::new(3, &[1], 4, POLICY);
    assert_eq!(planner.next_actions(), vec![send(1)]);
    planner.on_result(1, BlockResult::Acked);

    planner.on_session(&[1]);

    assert_eq!(planner.next_actions(), vec![send(1)]);
}

#[test]
fn a_plan_with_nothing_needed_commits_at_once_and_commits_once() {
    let mut planner = UploadPlanner::new(5, &[], 4, POLICY);

    assert_eq!(planner.next_actions(), vec![Action::Commit]);
    assert!(planner.next_actions().is_empty(), "the answer is awaited");
}

#[test]
fn commit_is_withheld_while_a_block_is_in_flight() {
    let mut planner = UploadPlanner::new(2, &all(2), 4, POLICY);
    planner.next_actions();
    planner.on_result(0, BlockResult::Acked);

    assert!(planner.next_actions().is_empty());
    planner.on_result(1, BlockResult::Acked);
    assert_eq!(planner.next_actions(), vec![Action::Commit]);
}

#[test]
fn a_listed_index_beyond_the_block_count_restarts_the_session() {
    let mut at_new = UploadPlanner::new(4, &[1, 4], 4, POLICY);
    let mut at_session = UploadPlanner::new(4, &all(4), 4, POLICY);
    at_session.on_session(&[9]);
    let mut at_result = UploadPlanner::new(4, &all(4), 4, POLICY);
    at_result.on_result(4, BlockResult::Acked);
    let mut at_commit = UploadPlanner::new(4, &all(4), 4, POLICY);
    ack_until_commit(&mut at_commit);
    at_commit.on_commit(CommitResult::NeedBlocks { list: vec![4] });

    for planner in [&mut at_new, &mut at_session, &mut at_result, &mut at_commit] {
        assert_eq!(planner.next_actions(), vec![Action::RestartSession]);
    }
}

#[test]
fn an_empty_need_blocks_answer_to_a_commit_restarts_the_session() {
    let mut planner = UploadPlanner::new(2, &all(2), 4, POLICY);
    ack_until_commit(&mut planner);

    planner.on_commit(CommitResult::NeedBlocks { list: vec![] });

    assert_eq!(planner.next_actions(), vec![Action::RestartSession]);
}

#[test]
fn inputs_that_do_not_fit_the_phase_are_ignored() {
    let mut planner = UploadPlanner::new(2, &all(2), 4, POLICY);
    planner.next_actions();

    planner.on_commit(CommitResult::Ok);
    planner.on_result(0, BlockResult::Acked);
    planner.on_result(1, BlockResult::Acked);
    assert_eq!(planner.next_actions(), vec![Action::Commit]);
    planner.on_session(&[0]);
    planner.on_result(0, conflict(Some(1)));
    planner.on_commit(CommitResult::Ok);
    assert_eq!(planner.next_actions(), vec![Action::Done]);
    planner.on_result(0, BlockResult::SessionGone);
    planner.on_session(&[1]);
    assert_eq!(planner.next_actions(), vec![Action::Done], "done is final");
}

#[test]
fn a_late_result_for_a_satisfied_block_changes_nothing() {
    let mut planner = UploadPlanner::new(3, &[2], 4, POLICY);

    planner.on_result(0, BlockResult::HashRejected);
    planner.on_result(1, conflict(Some(1)));

    assert_eq!(planner.next_actions(), vec![send(2)]);
}

#[test]
fn state_size_stays_small_for_a_large_plan() {
    let planner = UploadPlanner::new(25_600, &all(25_600), 4, POLICY);

    assert!(planner.approx_state_bytes() < 4 * 1024 * 1024);
}
