use std::time::Duration;

use crate::{MonotonicInstant, Timestamp};

/// True when wall time moved forward more than `gap` beyond what the monotonic clock measured between
/// two readings: the machine was suspended (or the wall clock was stepped forward) and the caller
/// should raise a `Resumed` signal. A jump of exactly `gap` is not a wake. A wall clock stepped backwards
/// is never a wake.
pub fn wake_gap_exceeded(
    prev_wall: Timestamp,
    prev_mono: MonotonicInstant,
    now_wall: Timestamp,
    now_mono: MonotonicInstant,
    gap: Duration,
) -> bool {
    let wall_micros =
        i128::from(now_wall.as_unix_micros()) - i128::from(prev_wall.as_unix_micros());
    let mono_micros = i128::try_from(now_mono.saturating_duration_since(prev_mono).as_micros())
        .unwrap_or(i128::MAX);
    let gap_micros = i128::try_from(gap.as_micros()).unwrap_or(i128::MAX);
    wall_micros.saturating_sub(mono_micros) > gap_micros
}
