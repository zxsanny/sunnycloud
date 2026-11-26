# Batch 128, loop 1: AZ-3878 split; AZ-4381 worker transfer hold (complete)

**Batch**: 128
**Tasks**: AZ-4381_worker_transfer_hold; the split of AZ-3878
**Date**: 2026-11-26

## Decision and split

D1, how the worker suite kills the supervisor mid-fetch and mid-post, was asked with a second opinion. The operator chose A: a one-shot hold on the API test image's `:9091` (`_docs/02_task_plans/az-3878-worker-suite/decisions.md`, `_docs/OPERATOR_TODO.md` D1).

AZ-3878 was larger than its 5 points. It is split into these tickets (specs in `todo/`, rows in `_dependencies_table.md`):

| Ticket | Task | Points |
|---|---|---|
| AZ-4381 | the hold | 3 |
| AZ-4382 | the fake decoder as a stack service | 5 |
| AZ-4383 | the H2 stack profile and ctl actions | 3 |
| AZ-4384 | IT-09-1, 3, 5, 8, 15 | 3 |
| AZ-4385 | IT-09-6, 7, 9 to 13 | 5 |

AZ-3878 keeps IT-09-2, 4, 14, 17 and 18, and now depends on AZ-4381 and AZ-4383. AZ-3879 now depends on AZ-3878, AZ-4384 and AZ-4385.

## What was built (AZ-4381)

- `components/11_http_api/src/test_hold.rs`, compiled only with `test-clock`:
  - `WorkerHold` holds one hold. Arming it moves it from idle to armed; the next matching stream takes it; once that stream has passed its bytes it is held.
  - `wrap()` leaves a body unchanged unless the armed route matches, and the kind if one was given. The matching stream passes exactly `after_bytes` bytes, splitting a chunk at the boundary, then waits for the release.
  - The wait uses `tokio::sync::Notify`, registered before the check so a release in between is not missed. It also ends after `HOLD_LIMIT` (60 s) of monotonic time; the offset clock plays no part.
- Routes on the `:9091` router, merged with `POST /test/clock`:
  - `POST /test/worker-hold`: arm; a second arm answers 400 `already_armed`;
  - `GET /test/worker-hold`: `{armed, held, job_id, bytes}`;
  - `DELETE /test/worker-hold`: release.
- Wiring, all under `#[cfg(feature = "test-clock")]`:
  - the `ApiServices.worker_hold` field, set by the server root;
  - the worker routes' state;
  - one `wrap` call in `open_input` (the answer body) and one in `put_output` (the request body).
- `futures-core` moved from the dev-dependencies to the dependencies, for the `Stream` impl. It was already in the lock.
- The release-symbol check gets `WorkerHold` as a marker.
- CI, features leg: `--test pipeline_worker_hold` joins the server test-clock step, and a new step runs `cargo test -p sunnycloud-http-api --features test-clock --lib test_hold`.

## AZ-4381 acceptance

| AC | Proof |
|---|---|
| AC-1 an input is held after N bytes | `server/tests/pipeline_worker_hold.rs` `ac1_...`: a 300 KB input stops at exactly 100,000 bytes. The status shows the job and 100,000. After the release the body equals the file |
| AC-2 an output is held, the API waits | `ac2_...`: the thumb post is held after 10 bytes. The job stays `running` with no derived row. The release gives 204 and one row |
| AC-3 one shot, matching only | `ac3_...` plus the unit test `the_hold_is_one_shot_and_takes_only_its_route_and_kind`: a preview passes while a thumb is armed; the second thumb passes; a second arm is refused |
| AC-4 a forgotten hold lets go | unit test `a_hold_nobody_releases_lets_go_at_its_limit`, with a 100 ms limit |
| AC-5 release builds have none of it | `check-release-symbols.sh`: the `test-clock` build of `sunnycloud` is flagged (`OffsetClock sc_clock6offset WorkerHold`, exit 1); the plain build is clean (exit 0) |

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4381_worker_transfer_hold | Done | 11: `test_hold.rs`, `test_hold/tests.rs`, `lib.rs`, `services.rs`, `server.rs`, worker `caller.rs`, `mod.rs`, `handlers.rs`, `Cargo.toml`; server `root/api.rs`, `tests/pipeline_worker_hold.rs`; CI | unit 4/4, black-box 3/3 (twice) | None |

## Code Review Verdict: PASS

- Every AC is traced, and no business logic is in the hold: it only meters bytes and waits.
- The default build's handlers keep their behaviour; the hold is a `cfg`-gated rebinding of the body.
- Spec correction: the second-arm refusal is 400 `invalid_request` (the validation style of `/test/clock`), not 409. The spec now says so.

## Test Suite

- `cargo test -p sunnycloud-http-api --features test-clock --lib test_hold`: 4 passed.
- `cargo test -p sunnycloud-server --features test-clock --test pipeline_worker_hold`: 3 passed, two runs.
- CI-parity: PASS.
  - Clippy with warnings as errors on `sunnycloud-http-api` and `sunnycloud-server`, all targets, with and without `test-clock`.
  - `rustfmt --check` on all changed Rust files.
  - `cargo test -p sunnycloud-http-api`, default build and with `test-clock`: 404 + 106 + 1 passed.
  - The CI list of `test-clock` server binaries: `test_clock_serve`, `pipeline_gc_retention`, `api_services`, `composition_root`, `pipeline_media_wire`, `pipeline_media_outputs`, `pipeline_worker_hold`, all green.
- First run (fixed before the commit):
  - the 11 test harness's `stub_services` lacked the new field under `test-clock`;
  - the route-scan test (`tests/it/registry/sources.rs`) needed `test_hold.rs` on its allow-list, next to `test_clock.rs`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A small output (a 34-byte WebP) sits whole in the socket buffers. When the API holds its read, the supervisor's write has already finished and it waits for the answer. A kill then is "during the post" as the API sees it: no answer has been sent and the job is not closed | AZ-3878 AC-6 | That is the moment AC-6 needs. Bigger outputs block the write itself | clear |
| 2 | When the supervisor is killed while an output is held, the API sees the broken connection only after the release, when it reads on | AZ-3878 AC-6 | The suite releases after the kill and then checks the end state, as the D1 card says | clear |

## Commit

`[AZ-4381] Worker transfer hold on the API test listener; AZ-3878 split` — Loop: 1

## Next Batch

AZ-4383 (H2 profile and ctl actions) and AZ-4382 (fake decoder service) are ready; AZ-3878 needs AZ-4383.
