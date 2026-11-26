# Batch 125, loop 1: AZ-3835 release-image leg (AZ-3835 complete); CI install stall fixed

**Batch**: 125
**Tasks**: AZ-3835_integration_suite_crate_graph (release-image leg of AC-2; AC-5 confirmed)
**Date**: 2026-11-26

## What was built

- The stack can carry the release api image: `E2E_API_VARIANT=release tests/run.sh --profile core` builds `deploy/docker/api.Dockerfile` with no feature (`org.sunnycloud.test-clock=false`) as `sunnycloud-api:e2e-release` and passes the variant to the consumer. The default stays the test image. The suites that move the test clock cannot run on the release image, so that run is narrowed with `E2E_PYTEST_ARGS=suites/http_api`.
- IT-11-9: `test_the_clock_endpoints_are_served_by_the_test_image_only` replaces the test-image-only clock test. On the test image, `:9091` `/test/clock` and `:8081` `/internal/v1/test/clock-offset` answer 200 with the same offset. On the release image, `:9091` refuses the connection and the clock-offset route is 404. The other IT-11-9 cases (worker routes 404 on `:8080` and through Caddy, 403 for every non-worker credential on `:8081`, the worker bearer reaches the handlers, ops paths only on `:9090`, clock routes not public) run unchanged on both images.
- `scripts/ci/install-tools.sh` `pg-client-18`: refreshes only the PGDG index (with apt timeouts and 3 retries). In the last two CI runs on `origin/dev` (45165398422, 45169525185, both on 2026-11-23), the full `apt-get update` hung on the runner's EC2 Ubuntu mirror until the 120 s watchdog aborted `install-pg-client` (exit 124). That failed `rust-test (3)` and `rust-test (features)` and skipped `coverage`. Not testable on this host (Ubuntu 26.04; the pin is for noble); the next CI run is the check.

## AZ-3835 acceptance

- AC-1: runner `scripts/ci/check-it-suite.sh` + `it_cases.txt`, job `http-api-suite`, built in batch 87.
- AC-2: both images, this batch.
- AC-3: IT-11-17 on the stack, batch 115.
- AC-4: `server/tests/crate_graph.rs`, batch 87.
- AC-5: in the last green CI run (45133395977, `03e8ea0`), `csrf` 100%, `auth` 98.7%, `problem.rs` 91.2%, `port_error` 99.6%, `content` 97.4%, crate 97.1%, all at or above their floors (90% / 75%).

AZ-3835 is archived; leftover `2026-11-18_az-3835-mode-r.md` is replayed and deleted.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3835_integration_suite_crate_graph | Done | 4 files (+ CI script) | `suites/http_api` 10/10 on the release image, 10/10 on the test image | None |

## Code Review Verdict: PASS

Self-review: the variant switch is one env value with two allowed spellings (`run.sh` exits 2 on any other). The compose defaults keep every existing run on the test image.

## Test Suite

- `E2E_API_VARIANT=release E2E_PYTEST_ARGS=suites/http_api tests/run.sh --profile core`: PASS=10 FAIL=0.
- `E2E_PYTEST_ARGS=suites/http_api tests/run.sh --profile core` (test image): PASS=10 FAIL=0.
- CI-parity: PASS (`bash -n` and shellcheck on `run.sh` and `install-tools.sh`; no Rust change).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | CI does not run the compose stack: no job calls `tests/run.sh` (environment.md asks for `core` on every PR and push to `dev`) | environment.md, CI/CD Integration | E-31 wiring; the stack suites run on a developer machine until then | clear |

## Next Batch

AZ-3847 (media integration suite), AZ-3878 (worker integration suite).
