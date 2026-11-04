# Helper: Clock (injectable time)

**Crate**: `sc_clock`. **Used by**: all server components (every `*_at` value, leases, expiry, retention, tombstone horizon), 05_helper_ids_hashing (UUIDv7 timestamps), 09_worker_supervisor, 14_desktop_daemon (settle, rescan, wake detection), 13_sync_core (backoff).

## Purpose
No component reads the system clock directly. Time comes through a trait so retention (7 days), tombstone horizon (90 and 91 days) and lease tests can advance days in seconds, and so client timing rules can be unit-tested without sleeping (AC-30).

## Interface

| Operation | Input | Output | Async | Errors |
|-----------|-------|--------|-------|--------|
| `Clock::now_utc` | none | `Timestamp` (UTC, microsecond) | No | none |
| `Clock::monotonic` | none | `MonotonicInstant` (never goes back, unaffected by suspend adjustments where the OS allows) | No | none |
| `Clock::sleep_until_monotonic` | `MonotonicInstant` | completes at the instant | Yes | `Cancelled` (shutdown) |
| `OffsetClock::advance` (test image only) | `Duration` | new offset | No | `ClockError::NegativeAdvance` |

Implementations:
- `SystemClock` (release): `now_utc` = OS time; monotonic = OS monotonic.
- `OffsetClock` (feature `test-clock`): `SystemClock` plus an atomic offset; `monotonic` is never offset, only wall time, so a day-long advance does not wake sleepers early and does not corrupt timeouts.
- `ManualClock` (unit tests, clients): both clocks advance only when the test calls `tick`.

## Test-image control endpoints (decided: decision 10 and `canonical-names.md`)
Two endpoints, both in the `-test` image only; the release image does not contain the routes, and CI asserts a 404 on the pushed release digest before signing:
- `POST /test/clock` with `{"advance_seconds": N}` (N >= 0) returns the new offset. It is served on the test-clock listener `:9091` and registered by 11_http_api only when the crate feature `test-clock` is compiled in (the `-test` image). This is how the harness sets the offset on the API. In release builds the route does not exist and answers 404 (FT-N-35); CI asserts it and also checks the release binary for the `test-clock` symbol.
- `GET /internal/v1/test/clock-offset` returns the current offset in seconds. It is served on the internal listener `:8081` behind the worker bearer secret and exists only in the `-test` image; the release image answers 404. The worker supervisor (also built with `test-clock` in its `-test` image) polls it at most once per second and applies the value to its own `OffsetClock`, so the API and every supervisor share one offset. No test-only table, no schema difference between images. Owner of the endpoints: 11_http_api; consumer: 09_worker_supervisor.

## Rules
- SQL never calls `now()`; the application passes `Timestamp` (data model). A grep test enforces it.
- Server order of commits is `account_feeds.seq`, never a timestamp; the clock decides only expiry and age questions (AC-8, NFT-RES-25).
- Durations come from 01_helper_config; the clock only measures them.
- Clients never trust the server clock for settle; settle uses the local monotonic clock. Wake detection: a wall-clock jump larger than `desktop.wake_gap` between two ticks of a monotonic timer raises a `Resumed` signal to 14_desktop_daemon.

## Tests
FT-N-35 (endpoints exist only in the test images), FT-P-16/17/18 (retention), FT-P-14 (91-day device), NFT-RES-09/10 (GC timing), NFT-RES-25 (clock skew), FT-P-52 (2 s settle with `ManualClock`), NFT-RES-16 (wake).

## Risks
- A test advancing wall time can expire sessions of the test client itself: tests mint tokens after the advance or use the clock-aware helper in the harness.
- `OffsetClock` leaking into a release build would let anyone with API access skip retention: guarded by the feature flag, the 404 test and the symbol check.
- The supervisor sees an advance up to one second late; tests that depend on it wait one poll interval after `POST /test/clock`.
