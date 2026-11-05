# Internal listener router (worker routes) and test-clock endpoints (feature `test-clock`)

**Task**: AZ-3831_internal_router_test_clock
**Name**: Internal router and test-clock endpoints
**Description**: The worker routes of 08 on the internal listener only, behind `WorkerAuthenticator`, with every other path there a 404; plus the test-image clock endpoints (`POST /test/clock` on `:9091`, `GET /internal/v1/test/clock-offset` on the internal listener) that exist only with feature `test-clock`
**Complexity**: 3 points
**Dependencies**: AZ-3803_body_limits_timeouts, AZ-3806_auth_extraction, AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3770_contracts_part3, AZ-3765_sc_clock
**Component**: 11 http_api
**Tracker**: AZ-3831
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `WorkerAuthenticator`, `WorkerJobs` (E-00/c05).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `OffsetClock::advance` and `offset` (E-03/c01).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — Worker and Test clock rows (E-21/c11).
- `_docs/02_document/contracts/11_http_api/http_server.md` — listeners (E-21/c03).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — 403, 404, 409, 413 and 422 bodies (E-21/c09).

## Problem

Workers talk only to the API (S4-1). Their routes hand out decrypted input and accept derived files, so a user or device credential must never reach them, and the public proxy must not expose them at all (AC-79, R12). Tests need to move server time (retention, share expiry, leases), and the supervisor must read the same offset, but a release build must not contain any way to do that (FT-N-35).

## Outcome

- The eight worker operations of 08 §5 are registered for the internal listener only and call one `WorkerJobs` method each.
- On the internal listener every path other than those routes (and the clock-offset read in test builds) is 404; the public router contains no worker or test route.
- With feature `test-clock`, `POST /test/clock` advances the shared `OffsetClock` and the worker-authenticated `GET /internal/v1/test/clock-offset` returns it; release builds contain neither, and `:9091` is not bound.

## Scope

### Included
- Worker routes: claims (200 `ClaimedJob` or 204), lease PUT and DELETE, input (exact `Content-Length`, streamed), outputs per kind (`WorkerOutput` class), result, failure, status; `Cache-Control: no-store`; `X-Worker-Id` passed through.
- Mapping of `LeaseLost` (409 `lease_lost`), `RevisionObsolete` (404), output outcomes (204 `Accepted`, 200 `AlreadyPresent`, 413, 422).
- The 404 fallback of the internal router.
- The two test-clock endpoints and the `:9091` listener behind feature `test-clock`.

### Excluded
- Queue, lease and output rules (08); the supervisor client (E-20/c03) and its offset poll (E-20/c09).
- The release-image 404 check in CI and the Caddy run (E-21/c19, E-31/c07, pipeline AZ-3660).

## Acceptance Criteria

**AC-1: Worker routes exist only on the internal listener (AC-79)**
Given the public and internal routers of one build
When `POST /api/v1/worker-jobs/claims`, `GET /api/v1/worker-jobs/{id}/input` and `PUT /api/v1/worker-jobs/status` are sent to the public listener
Then each answers 404 with the same body as an unknown path

**AC-2: User credentials get one uniform 403 (AC-79)**
Given the internal listener
When each of the eight worker operations is called without `Authorization`, with a session bearer, a session cookie, a device token and a wrong worker secret
Then all 40 answers are 403 with bodies equal apart from `request_id`, and no `WorkerJobs` method is called

**AC-3: Worker bearer reaches the operations**
Given the worker bearer and `WorkerJobs` stubs
When each operation is called
Then each calls its one method with the job id, worker id and body, `claims` answers 200 with the job or 204 when the stub has none, `LeaseLost` is 409 `lease_lost`, `RevisionObsolete` is 404, `Accepted` is 204 and `AlreadyPresent` is 200

**AC-4: Only worker routes on the internal listener (G-11-3)**
Given the internal listener
When public API paths such as `GET /api/v1/nodes/root`, `GET /api/v1/server-info` and `/` are requested with any credential
Then each answers 404

**AC-5: Test clock works in the test build (FT-N-35)**
Given a build with feature `test-clock`
When `POST /test/clock` on `:9091` advances by 691200 seconds, then `GET /internal/v1/test/clock-offset` is read with the worker bearer and without it
Then the advance is accepted, the read with the worker bearer returns an offset of 691200 seconds, and the read without it is 403

**AC-6: Release build has no clock control (FT-N-35)**
Given a build without feature `test-clock`
When `:9091` is connected to, `GET /internal/v1/test/clock-offset` is called on the internal listener with the worker bearer, and `POST /test/clock` is sent to the public listener with and without an admin session
Then the connection to `:9091` is refused and every request answers 404

## Non-Functional Requirements

**Security**
- The internal listener binds only the configured backend address (`internal_addr`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-4 | Router tables of both listeners | Worker routes only internal; internal holds nothing else |
| AC-6 | Build without `test-clock` | No clock route symbols and no `:9091` binding in the route table |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S | Worker routes on public (IT-11-9, Mode S part) | 404 like an unknown path | AC-79, NFT-SEC-50 step 4 |
| AC-2 | Mode S, 5 credentials x 8 operations | Uniform 403 (IT-11-9) | 40 of 40 equal 403 | AC-79, NFT-SEC-50 step 1 |
| AC-3 | Mode S, worker bearer | Operation mapping | Statuses as listed | AC-71 |
| AC-4 | Mode S | Public paths on internal | 404 | G-11-3 |
| AC-5 | Mode S, `test-clock` build | Advance and read | Accepted; offset equal; 403 without bearer | FT-N-35, AC-30 |
| AC-6 | Mode S, release build | Clock routes absent | Refused or 404 | FT-N-35 |

## Constraints

- No worker credential or scope exists in 01; worker auth is 08's `WorkerAuthenticator` only.

## Risks & Mitigation

**Risk 1: Worker routes leak onto the public router**
- *Risk*: a shared registry mounts them on both.
- *Mitigation*: listener placement is a registry fact (E-21/c11 AC-6); AC-1 here; pipeline AZ-3660 through Caddy.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Auth on `POST /test/clock` (`:9091`): `observability.md` says "bearer via `e2e-ctl`", but no such credential is defined in 11 or helper 01 | E-21, E-31 owners | open | Medium |
| `POST /test/clock` advances an offset (`sc_clock`, FT-N-35 body `advance_seconds`) while 11 §4 says it "sets" the offset; this task follows `sc_clock` (non-negative, only grows) | E-03, E-21 owners | open | Low |
| `Range` on the worker input route (G-08-4) is still a spike (E-18/c01) | E-18 owner | open | Low |
| The worker job DTOs are declared both in `contracts` and in `media-shared` (part 3 open point 2); the wire test keeping them equal is E-18/E-20's | E-18, E-20 owners | open | Low |

## Contract

This task implements the Worker and Test clock rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
