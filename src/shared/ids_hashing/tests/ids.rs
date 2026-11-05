use std::time::Duration;

use sc_clock::{ManualClock, Timestamp};
use sc_ids::new_id;
use uuid::{Variant, Version};

#[test]
fn new_id_is_v7_and_sorts_after_a_tick() {
    let clock = ManualClock::new(Timestamp::from_unix_micros(1_800_000_000_000_000));

    let before = new_id(&clock);
    clock.tick(Duration::from_millis(5));
    let after = new_id(&clock);

    assert_eq!(before.get_version(), Some(Version::SortRand));
    assert_eq!(before.get_variant(), Variant::RFC4122);
    assert!(before < after, "{before} must sort before {after}");
}

#[test]
fn new_id_carries_the_clock_millisecond_and_a_random_tail() {
    let clock = ManualClock::new(Timestamp::from_unix_micros(1_800_000_000_123_456));

    let first = new_id(&clock);
    let second = new_id(&clock);

    let expected_millis = 1_800_000_000_123u64;
    assert_eq!(first.as_u128() >> 80, u128::from(expected_millis));
    assert_ne!(first, second, "random tail differs inside one millisecond");
}
