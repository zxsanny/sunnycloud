# One-shot worker transfer hold on the API test listener

**Task**: AZ-4381_worker_transfer_hold
**Name**: Worker transfer hold (test image)
**Description**: In the API test image, park the next matching worker input or output body stream after N bytes until the test releases it, so a supervisor kill lands inside the fetch or inside the post
**Complexity**: 3 points
**Dependencies**: AZ-3831_internal_router_test_clock, AZ-3843_worker_auth_jobs_facade, AZ-4334_serve_test_clock_wiring
**Component**: 11_http_api
**Tracker**: AZ-4381
**Epic**: AZ-3644 (E-21)
**Ref**: split from AZ-3878 (E-20/c11); decision D1 (`_docs/02_task_plans/az-3878-worker-suite/decisions.md`, operator 2026-11-26)

## Problem

IT-09-14 (AZ-3878 AC-6) kills the supervisor during the input fetch and during the output post. On the test stack both take milliseconds, and the API logs no line for a 2xx answer (`components/11_http_api/src/middleware/observe.rs`), so nothing marks the moment to kill.

## Outcome

- The `:9091` listener of a `test-clock` build answers three routes, with no credential, like `POST /test/clock`:
  - `POST /test/worker-hold {route: "input" | "output", kind?: "thumb" | "preview" | "transcode", after_bytes}`: arms one hold. 204. A second arm while one is armed or held answers 400 `invalid_request` with the field problem `route: already_armed`, the validation style of `POST /test/clock`.
  - `GET /test/worker-hold`: answers `{armed, held, job_id, bytes}`.
  - `DELETE /test/worker-hold`: releases a held stream and disarms. 204.
- The next worker input (`GET .../input`) or output (`PUT .../outputs/{kind}`) that matches passes exactly `after_bytes` bytes and then stops until released. The hold is one shot: later transfers pass unchanged. A hold left unreleased lets the stream go after 60 s of monotonic time, so a forgotten hold cannot block the API's stop. The test clock plays no part.
- Release builds contain none of it. `scripts/ci/check-release-symbols.sh` gets the hold type's name as a marker.

## Scope

### Included
- The hold state and the stream wrapper in 11 under `#[cfg(feature = "test-clock")]`, the three routes on the `:9091` router, and the field on `ApiServices` set by the server root.
- Tests through the real `sunnycloud serve` binary built with `test-clock`.

### Excluded
- The suite that uses it (AZ-3878), any change to the supervisor, any route on the public, internal or ops listeners.

## Acceptance Criteria

**AC-1: An input stream is held after N bytes**
Given a claimed job with a 300 KB input and a hold armed on `input` with `after_bytes` 100,000
When the worker reads its input
Then exactly 100,000 bytes arrive and the stream stops; `GET /test/worker-hold` shows `held: true` with that job and 100,000 bytes; after `DELETE` the remaining bytes arrive and the body equals the file

**AC-2: An output stream is held, and the API waits for it**
Given a claimed stills job and a hold armed on `output` and kind `thumb`
When the worker posts its thumb
Then the API stops reading after N bytes, the job stays `running` with no derived row while held, and after release the post answers 204

**AC-3: One shot, matching only**
Given an armed hold on `output` and kind `thumb`
When a `preview` is posted, then a `thumb`, then a second `thumb` of another job
Then the preview and the second thumb pass unchanged and only the first thumb is held

**AC-4: A forgotten hold lets go**
Given a held stream nobody releases
When 60 s of monotonic time pass (shortened through a constructor argument in the test)
Then the stream continues, and the status shows nothing held

**AC-5: Release builds have none of it**
Given the release API image
When `check-release-symbols.sh` scans it
Then the hold's marker is absent; `:9091` is not served

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-3, AC-4 | The stream wrapper on an in-memory stream | Splits a chunk at the boundary; one shot; route and kind match; releases at the time limit |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior |
|--------|------------------------|-------------|-------------------|
| AC-1, AC-2, AC-3 | `serve` with `test-clock`, a committed photo | `/test/worker-hold` with the worker routes over HTTP | As the ACs |
| AC-5 | release image | `check-release-symbols.sh` | marker absent |

## Constraints

- No business logic in the hold: it only meters bytes and waits. The worker routes call it at one place each.
- The timeout is monotonic time, never the offset clock: moving the test clock must not release a hold.
