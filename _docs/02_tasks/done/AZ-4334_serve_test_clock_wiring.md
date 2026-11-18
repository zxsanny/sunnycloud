# Wire feature test-clock in the serve binary

**Task**: AZ-4334_serve_test_clock_wiring
**Name**: serve test clock
**Description**: Make the server's feature `test-clock` build one `OffsetClock` for `serve`, give it to every component as the `Clock` and to 11 as the test-clock handle, and bind the test-clock listener, as `http_server.md` describes; release builds stay free of both
**Complexity**: 3 points
**Dependencies**: AZ-3831_internal_router_test_clock, AZ-3789_composition_root, AZ-3791_serve_start_stop
**Component**: 12 (server composition), with 11's existing test-clock router
**Tracker**: AZ-4334
**Epic**: AZ-3647 (E-22)
**Ref**: found in batch 87 (AZ-3835 Mode R leftover); prerequisite of AZ-3663 (owner decision 2026-11-18)

## Problem

`src/server/Cargo.toml` declares `test-clock = []`. The `-test` images (`scripts/ci/build-images.sh`, `FEATURES=test-clock`) therefore get neither `sunnycloud-http-api/test-clock` (the `:9091` listener with `POST /test/clock`, and `GET /internal/v1/test/clock-offset` on the internal listener) nor `sc_clock/test-clock`, and `src/server/src/wiring.rs` always builds `SystemClock`. No end-to-end test can move the server's time, and the test image serves no clock endpoint.

## Acceptance Criteria

**AC-1: Feature build moves time**
Given a `serve` binary built with feature `test-clock`
When a test posts an offset to the test-clock listener and then triggers a time-based decision of the server (for example an upload session past its TTL, reaped by the scheduler)
Then the decision follows the moved clock, and `GET /internal/v1/test/clock-offset` with the worker bearer reports the offset

**AC-2: One clock**
Given the feature build
When the composition root is built
Then every component that takes a `Clock` holds the same `OffsetClock` instance the test-clock router moves

**AC-3: Release build unchanged**
Given the default build
When `serve` starts
Then nothing listens on the test-clock address, `/internal/v1/test/clock-offset` answers 404, and the release-image check (`deploy/tests/check-images.sh`, no test-clock symbol) still passes

**AC-4: CI**
Given CI
When the server tests run
Then the feature-build tests of AC-1 and AC-2 run in a CI step with `--features test-clock`

## Constraints

- Thin composition only (ADR-015): no business rule in 12; 11's router and `sc_clock`'s `OffsetClock` are reused, not reimplemented.
- The test-clock listener address key follows `http_server.md` open point 4 (default `0.0.0.0:9091`, field present only under the feature); if the config key does not exist yet, add it the way that open point describes and record it.

## Implementation notes (2026-11-18)

- Server feature `test-clock` = `sunnycloud-http-api/test-clock`, `sc_clock/test-clock`, `sc_config/test-clock`. `wiring::process_env` builds one `OffsetClock` and calls `root::register_with_test_clock`, which registers it as `dyn Clock` (every builder) and as `OffsetClock` (`ApiServices.offset_clock`).
- Key `test_clock_addr` (default `0.0.0.0:9091`, reader api) is a `#[cfg(feature = "test-clock")]` row of `sc_config`'s table; a default binary refuses `SUNNYCLOUD_TEST_CLOCK_ADDR` as unknown. `http_server.md` open point 4 closed; the row is still to be added to `common-helpers/01_helper_config.md`.
- Tests: `src/server/tests/test_clock_serve.rs` (feature module: AC-1, AC-2; default module: AC-3). AC-4: CI step `cargo test --locked -p sunnycloud-server --features test-clock --test test_clock_serve`.
