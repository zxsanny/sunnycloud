# Contract: sc_clock — injectable time

**Component**: shared/clock (crate `sc_clock`; feature `test-clock` on the API and worker crates only)
**Producer task**: E-03/c01 — `_docs/02_tasks/todo/B01-09_sc_clock.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-04/c01, E-05/c01, E-06/c01a, E-07/c01, E-08/c01, E-09/c01, E-00/c03..c05 (`Timestamp` in port signatures); other batches: every server component, 09, 13, 14
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

No component reads the system clock directly. Wall time and monotonic time come through one trait, so retention, tombstone horizon and lease tests advance days in seconds and client timing rules run without sleeping.

Sources: `common-helpers/03_helper_clock.md`, `epics/E-03_cross-cutting-clock.md`, `data_model.md` §1 (Time).

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `Clock::now_utc` | `fn now_utc(&self) -> Timestamp` (UTC, microsecond) | none | sync, lock-free |
| `Clock::monotonic` | `fn monotonic(&self) -> MonotonicInstant` (never goes back) | none | sync, lock-free |
| `Clock::sleep_until_monotonic` | `async fn sleep_until_monotonic(&self, at: MonotonicInstant) -> Result<(), Cancelled>` | `Cancelled` on shutdown | async |
| `SystemClock::new` | release clock: OS wall and OS monotonic | none | |
| `OffsetClock::advance` (feature `test-clock`) | `fn advance(&self, by: Duration) -> Result<Duration, ClockError>` (returns the new offset) | `ClockError::NegativeAdvance` | sync |
| `OffsetClock::offset` (feature `test-clock`) | `fn offset(&self) -> Duration` | none | sync |
| `ManualClock::tick` | `fn tick(&self, by: Duration)` (moves wall and monotonic together) | none | sync |
| `wake_gap_exceeded` | `fn wake_gap_exceeded(prev_wall, prev_mono, now_wall, now_mono, gap: Duration) -> bool` | none | sync, pure |

| Type | Shape |
|---|---|
| `Timestamp` | UTC instant with microsecond precision; maps to `timestamptz` |
| `MonotonicInstant` | opaque monotonic reading |
| `Cancelled`, `ClockError` | error values above |

## Invariants

- `OffsetClock` offsets wall time only; `monotonic` is never offset, so an advance never wakes a sleeper early. The offset is non-negative and only grows.
- `ManualClock` time moves only on `tick`; sleepers on it wake only when a `tick` passes their instant.
- SQL never calls `now()`: a grep gate over `*.sql` files and query strings fails on any occurrence (rule owned here).
- Ordering is never decided by a timestamp (server order is `account_feeds.seq`).
- The release binaries contain neither the `test-clock` feature nor its symbols (release-binary check).

## Non-Goals

- Not covered: the two HTTP endpoints (`POST /test/clock` on `:9091`, `GET /internal/v1/test/clock-offset` on `:8081`) owned by E-21, and their 404 check on the release image; the supervisor's offset poll (E-20); duration values (`sc_config`).

## Versioning Rules

- Adding a method with a default body is minor; changing `Timestamp` precision or `Clock` method signatures is breaking.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| eight-days | `ManualClock` + retention predicate, `tick(8d)` | predicate flips, test takes < 1 s | AC-30 |
| no-early-wake | `OffsetClock::advance(1 day)` then a 10 s monotonic sleep | sleep still takes 10 s | |
| negative | `advance(-1s)` | `NegativeAdvance` | |
| cancel | shutdown during `sleep_until_monotonic` | `Cancelled` within 100 ms | |
| sql-now | seeded `now()` in an SQL file | grep gate fails | |
| wake-gap | readings with a wall jump just above / below the gap | `true` / `false` | |

## Open points (flagged in the producer task)

1. How `sleep_until_monotonic` learns about shutdown is not specified (a cancellation token passed in, or one held by the clock).
2. The `desktop.wake_gap` key is not in the config table; `wake_gap_exceeded` takes the gap as an argument and E-24 supplies it.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
