# sc_clock: injectable clock with system, offset and manual implementations

**Task**: AZ-3765_sc_clock
**Name**: sc_clock `Clock`, system/offset/manual clocks, wake-gap predicate, `now()` and symbol gates
**Description**: One clock trait for wall time, monotonic time and cancellable sleeps, with a release clock, an offset clock for the `-test` images and a manual clock for tests, plus the gates that keep SQL `now()` and the test clock out of production
**Complexity**: 3 points
**Dependencies**: AZ-3761_ci_skeleton, AZ-3652_initial_structure
**Component**: shared/clock (helper 03)
**Tracker**: AZ-3765
**Epic**: AZ-3630 (E-03)
**Ref**: E-03/c01

## Problem

Seven-day retention, the 90-day tombstone horizon and share expiry must be testable in seconds (AC-30), the 2 s settle rule must run without sleeping (AC-18), and ordering must never depend on a timestamp (AC-8). If any code reads the system clock or SQL calls `now()`, the test clock cannot move it; if the test clock reaches a release image, anyone with API access could skip retention.

## Outcome

- Every time question goes through one trait; tests advance days instantly and deterministically.
- The `-test` images can shift wall time by an offset without waking sleepers early.
- A seeded SQL `now()` or a release binary containing the test clock fails CI.

## Scope

### Included
- `Timestamp` (UTC, microseconds) and `MonotonicInstant`; the clock trait with `now_utc`, `monotonic`, `sleep_until_monotonic` (cancellable, `Cancelled` on shutdown).
- `SystemClock`; `OffsetClock` behind the `test-clock` feature (atomic non-negative offset on wall time only, `advance` with `NegativeAdvance`, read of the current offset); `ManualClock` (both clocks move only on `tick`).
- The pure wake-gap predicate.
- The SQL `now()` grep gate over SQL files and query strings (this rule is owned here; E-06 consumes it).
- The release-binary check that the `test-clock` feature symbols are absent from the release api and worker binaries, run in the `images` CI job.
- Crate documentation of which clock each layer uses.

### Excluded
- The HTTP endpoints `POST /test/clock` and `GET /internal/v1/test/clock-offset` and their 404 check (E-21); the supervisor's offset poll (E-20); the daemon timer loop and settle logic (E-24); duration values (E-01).

## Acceptance Criteria

**AC-1: Time advances without sleeping (AC-30)**
Given a retention check driven by `ManualClock`
When the test ticks the clock by 8 days
Then the check flips from "keep" to "due" and the test finishes in under 1 s

**AC-2: Sleepers not woken early**
Given an `OffsetClock` and a sleeper waiting 10 s on the monotonic clock
When the offset is advanced by 1 day during the sleep
Then the sleep still ends after 10 s and `monotonic` is unchanged by the advance

**AC-3: Offset rules**
Given an `OffsetClock`
When it is advanced by 5 s, then by -1 s
Then the first call returns the new offset of 5 s, the second returns `NegativeAdvance` and the offset stays 5 s

**AC-4: Release has no test clock (FT-N-35 part)**
Given the release api and worker binaries built by the `images` job
When the release-binary check runs, and again on a branch that enables `test-clock` in the release build
Then the first run finds no `test-clock` symbol and the second fails

**AC-5: No SQL `now()`**
Given the repository
When the grep gate runs, and again with a seeded `now()` in an SQL file
Then it finds 0 occurrences, then fails on the seeded one

**AC-6: Settle rule testable (AC-18)**
Given a 2 s settle rule driven by `ManualClock`
When the test ticks 1.9 s, then 0.2 s
Then the rule fires only after the second tick, with no real sleep (FT-P-52 unit part)

**AC-7: Shutdown cancels sleeps**
Given a sleeper on `sleep_until_monotonic` for 1 hour
When shutdown is signalled
Then the sleep returns `Cancelled` within 100 ms

**AC-8: Wake gap**
Given two readings whose wall-clock jump is the gap plus 1 ms, and two whose jump is the gap minus 1 ms
When `wake_gap_exceeded` evaluates them
Then the first is `true` and the second `false`

## Non-Functional Requirements

**Performance**
- `now_utc` and `monotonic` are lock-free reads with no allocation.

**Security**
- The offset clock and its symbols exist only in builds with the `test-clock` feature (API and worker `-test` images).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `ManualClock` tick of 8 days | Predicate flips; no real wait |
| AC-2 | `OffsetClock` advance during a monotonic sleep | Sleep length unchanged |
| AC-3 | Positive and negative advance | New offset; `NegativeAdvance` |
| AC-6 | Settle rule boundary with `ManualClock` | Fires only after 2 s |
| AC-7 | Cancel during sleep | `Cancelled` within 100 ms |
| AC-8 | Wake-gap boundaries | `true` / `false` |
| AC-5 | Grep gate on a seeded file | Fails |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-4 | Release images (with E-21 routes) | FT-N-35, NFT-SEC-38 step 4 | Control endpoints answer 404; no test-clock symbol | Security |
| AC-1 | Test stack with the offset clock (through E-10) | FT-P-16, FT-P-17, FT-P-18 | Retention behaves as configured after an advance | — |
| AC-1 | Test stack, device idle 91 days (through E-15) | FT-P-14 | Device handled per the 90-day rule | — |
| AC-2 | GC timing under advances (through E-10) | NFT-RES-09, NFT-RES-10 | GC runs as configured | — |
| AC-1 | Client clock skew | NFT-RES-25 | Server order decides | — |
| AC-8 | Daemon suspend and resume (through E-24) | NFT-RES-16 | `Resumed` raised after the gap | — |

## Constraints

- The application passes `Timestamp` values; SQL never calls `now()` (data_model §1).
- Ordering is by `account_feeds.seq`, never a timestamp.
- `tokio` time only behind the trait; no `chrono` in the public API beyond conversion.

## Risks & Mitigation

**Risk 1: Offset clock leaks into a release**
- *Risk*: a release image exposes clock control.
- *Mitigation*: feature flag, release-binary symbol check here, 404 check on the pushed digest (E-21, release workflow).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How `sleep_until_monotonic` learns about shutdown (token passed per call, or held by the clock) is not specified. | E-03 owner | open | Low |
| `desktop.wake_gap` is named in helper 03 but missing from the config table; the predicate takes the gap as an argument. | E-01 / E-24 owners | open | Low |
| Both E-03 and E-06 list the SQL `now()` grep gate; it is built once, here. | E-03 owner | resolved | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_clock/sc_clock.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
