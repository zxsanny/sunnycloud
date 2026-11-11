# Batch Report

**Batch**: 32
**Tasks**: AZ-3831_internal_router_test_clock
**Date**: 2026-11-11

One worker on `11_http_api`. Every other ready task was in the same crate (AZ-3828, AZ-3829, AZ-3830, AZ-3833, one at a time) or Windows only (AZ-3893) or a decision spike; AZ-3829 (5 points) was left for a batch of its own because it needs the complexity-budget question first. No shared crate was edited.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3831_internal_router_test_clock | Done; the literal refusal of a connection to `:9091` is not probed | `sunnycloud-http-api`: `registry/worker/` (the eight worker operations, internal listener only), `test_clock.rs` (feature `test-clock`: `GET /internal/v1/test/clock-offset`, `POST /test/clock` on `:9091`), `json_body.rs`, `ApiProblem::validation`, listener wiring; `api_v1_surface.md` 1.3.0, `http_server.md` 1.3.0, `problem_details.md` 1.0.2 | 202 `it` + 55 unit (default features), 204 `it` with `test-clock`; new `tests/it/worker/{support,listeners,operations,outputs,clock}.rs` | Items 1-8 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the new code; no file over 500 lines. Eight existing tests were adapted because production now has worker routes (`registry/support.rs`, `placement.rs`, `parity.rs`, `csrf/reads.rs`, `naming.rs`, `sources.rs`, `harness`). Warnings: the version gate can break the uniform 403 on worker routes (item 1), and `POST /test/clock` has no credential (item 2).

CI-parity: all gates passed except `cargo deny` (item 9). In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. The worker also ran the crate's tests and clippy with `--features test-clock`. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2623 (workspace, default features, real PostgreSQL 18)
- Passed: 2614
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None asked this batch. Waiting for the owner: items 1, 2, 3, 4, 9.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | The version gate runs before auth on `Worker` routes (an existing test requires it). With `min_client_version` set, a worker call without `X-Sunnycloud-Client` gets 400, and one below the minimum gets 426, before the uniform 403. That breaks AC-2/AC-79 in that configuration: either the supervisor sends the header or the gate skips `Worker` routes | AZ-3831 AC-2 | Nothing changed; owner decision (same open item as the earlier `Worker` and `ShareGrant` version-gate item) | unclear |
| 2 | `POST /test/clock` has no credential: the documents define none (api_v1_surface open point 7). It sits on a test-image-only listener no proxy routes, but anyone reaching `:9091` can move server time. Owner defines the `e2e-ctl` credential or confirms the network boundary is enough | AZ-3831 | Implemented with none | unclear |
| 3 | Clock wire shape is the worker's choice: request `{"advance_seconds": N}` (non-negative integer, otherwise 400), answer and the offset read `{"offset_seconds": <whole seconds>}`; follows `sc_clock` (advance, grow-only), not "sets". The supervisor poll (AZ-3875) must read this shape | AZ-3831 AC-5 | Recorded in `api_v1_surface.md` open points 17-21 | unclear |
| 4 | `WorkerOutput` uses `request_timeout` (60 s) as its total handler time, so a large transcode upload is cut at 60 s; it probably should follow `Block` and `Staged` (idle timer, no total cap). That is AZ-3803's decision | AZ-3803 item 8 | Left | unclear |
| 5 | Worker input rules taken by the simplest reading: `X-Worker-Id` once and non-empty (no length rule), a `job_id` that is not a UUID is 400, bodies need exactly the 08 section 5 fields, the failure `code` is passed through unvalidated; an unknown output `{kind}` is 422 `output_kind_rejected` with no port call; `Content-Length` and `Content-Type` are required on outputs (chunked gives 411); non-numeric `X-Width`, `X-Height`, `X-Duration-Ms` is 400 | AZ-3831 | Worker's reading | unclear |
| 6 | A route declares one success status, so the OpenAPI document shows only the first-named outcome for `claims` and `outputs` | AZ-3831 | Worker's reading | unclear |
| 7 | AC-6 (release build): the literal connection refusal to `:9091` is not probed (a host port is flaky to test). Covered by an empty clock route table, no `clock` in the document, 404 for the offset read and for `POST /test/clock` on public with and without admin | AZ-3831 AC-6 | Worker's reading | clear |
| 8 | Left for the composition root (AZ-3789): set `ApiServices.worker_auth`, `worker_jobs` and, with the feature, `offset_clock` (the same instance as `clock`) and `HttpConfig.test_clock_addr`; there is no `sc_config` key for the address yet (suggested default `0.0.0.0:9091`); handle the new `Listener::TestClock` bind error; the release-binary symbol check and the 404 check on the release image are outside this crate | AZ-3831 | For AZ-3789 | clear |
| 9 | `cargo deny check` fails with RUSTSEC-2026-0459 (`libsqlite3-sys` 0.37.0 through `rusqlite` 0.39.0 in `sunnycloud-daemon`; fix is 0.39.0 or later). It is outside this batch and was reported in batch 31 | gate | Owner decision | unclear |
| 10 | Items open from batches 18-31 stay open, including the missing `ETag` on the 416 | earlier batches | | unclear |

## Commit

`[AZ-3831] Internal router for worker routes and test-clock endpoints` — Loop: 1

## Next Batch

Recompute ready tasks. Only `11_http_api` tasks are ready besides Windows and spikes: AZ-3833 graceful shutdown (2 points), AZ-3830 share and media handlers (3), AZ-3828 identity admin handlers (5), AZ-3829 tree, transfer and feed handlers (5). 255 tasks left.
