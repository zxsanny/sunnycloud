//! Timestamp arithmetic: microsecond precision, checked, signed epoch.

use std::time::Duration;

use sc_clock::{Clock, SystemClock, Timestamp};

#[test]
fn micros_round_trip_and_order() {
    let a = Timestamp::from_unix_micros(-5);
    let b = Timestamp::from_unix_micros(7);
    assert_eq!(-5, a.as_unix_micros());
    assert!(a < b);
}

#[test]
fn arithmetic_truncates_to_microseconds_and_checks_overflow() {
    let t = Timestamp::from_unix_micros(1_000);
    assert_eq!(
        Some(Timestamp::from_unix_micros(1_002)),
        t.checked_add(Duration::from_nanos(2_999))
    );
    assert_eq!(
        Some(Timestamp::from_unix_micros(998)),
        t.checked_sub(Duration::from_nanos(2_999))
    );
    assert_eq!(
        None,
        Timestamp::from_unix_micros(i64::MAX).checked_add(Duration::from_micros(1))
    );
    assert_eq!(
        None,
        Timestamp::from_unix_micros(i64::MIN).checked_sub(Duration::from_micros(1))
    );
    assert_eq!(None, t.checked_add(Duration::MAX));
}

#[test]
fn duration_since_is_none_when_earlier_is_later() {
    let early = Timestamp::from_unix_micros(10);
    let late = Timestamp::from_unix_micros(25);
    assert_eq!(
        Some(Duration::from_micros(15)),
        late.checked_duration_since(early)
    );
    assert_eq!(Some(Duration::ZERO), late.checked_duration_since(late));
    assert_eq!(None, early.checked_duration_since(late));
}

#[test]
fn system_clock_reads_current_time_and_never_goes_back() {
    let clock = SystemClock::new();
    let (wall, mono) = (clock.now_utc(), clock.monotonic());
    // 2020-01-01 in microseconds: any real machine is later.
    assert!(wall.as_unix_micros() > 1_577_836_800_000_000);
    assert!(clock.monotonic() >= mono);
}
