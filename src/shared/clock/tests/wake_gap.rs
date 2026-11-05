//! AC-8: the pure wake-gap predicate.

use std::time::Duration;

use sc_clock::{Clock, ManualClock, MonotonicInstant, Timestamp, wake_gap_exceeded};

const GAP: Duration = Duration::from_secs(30);
const MS: Duration = Duration::from_millis(1);
const WALL0: Timestamp = Timestamp::from_unix_micros(1_700_000_000_000_000);

fn mono(clock: &ManualClock, after: Duration) -> MonotonicInstant {
    clock.monotonic().checked_add(after).expect("no overflow")
}

#[test]
fn ac8_gap_plus_one_ms_is_true_and_gap_minus_one_ms_is_false() {
    let clock = ManualClock::new(WALL0);
    let mono0 = clock.monotonic();
    let over = WALL0.checked_add(GAP + MS).expect("no overflow");
    let under = WALL0.checked_add(GAP - MS).expect("no overflow");

    assert!(wake_gap_exceeded(WALL0, mono0, over, mono0, GAP));
    assert!(!wake_gap_exceeded(WALL0, mono0, under, mono0, GAP));
}

#[test]
fn a_jump_of_exactly_the_gap_is_not_a_wake() {
    let clock = ManualClock::new(WALL0);
    let mono0 = clock.monotonic();
    let exact = WALL0.checked_add(GAP).expect("no overflow");

    assert!(!wake_gap_exceeded(WALL0, mono0, exact, mono0, GAP));
}

#[test]
fn monotonic_time_that_really_passed_is_not_a_jump() {
    let clock = ManualClock::new(WALL0);
    let mono0 = clock.monotonic();
    let ten_minutes = Duration::from_secs(600);

    // Ten idle minutes on both clocks: no suspend happened.
    let wall1 = WALL0.checked_add(ten_minutes).expect("no overflow");
    assert!(!wake_gap_exceeded(
        WALL0,
        mono0,
        wall1,
        mono(&clock, ten_minutes),
        GAP
    ));

    // Ten minutes of wall time but one second of monotonic time: the machine slept.
    assert!(wake_gap_exceeded(
        WALL0,
        mono0,
        wall1,
        mono(&clock, Duration::from_secs(1)),
        GAP
    ));
}

#[test]
fn a_wall_clock_stepped_backwards_is_not_a_wake() {
    let clock = ManualClock::new(WALL0);
    let mono0 = clock.monotonic();
    let earlier = WALL0
        .checked_sub(Duration::from_secs(3_600))
        .expect("no overflow");

    assert!(!wake_gap_exceeded(WALL0, mono0, earlier, mono0, GAP));
}

#[test]
fn extreme_readings_do_not_overflow() {
    let clock = ManualClock::new(WALL0);
    let mono0 = clock.monotonic();
    let lowest = Timestamp::from_unix_micros(i64::MIN);
    let highest = Timestamp::from_unix_micros(i64::MAX);

    assert!(wake_gap_exceeded(lowest, mono0, highest, mono0, GAP));
    assert!(!wake_gap_exceeded(highest, mono0, lowest, mono0, GAP));
    assert!(!wake_gap_exceeded(
        WALL0,
        mono0,
        highest,
        mono0,
        Duration::MAX
    ));
}
