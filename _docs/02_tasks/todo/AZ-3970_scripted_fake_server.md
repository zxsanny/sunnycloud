# Scripted fake HTTP server fixture (drop, bit flip, delay, 401/410/422/426/429)

**Task**: AZ-3970_scripted_fake_server
**Name**: Scripted fake server fixture
**Description**: An in-process, test-only HTTP/2 server that answers the `/api/v1` routes the sync core calls with scripted responses and faults, logs every request, and lets every E-23 executor task test its protocol rules without the real server stack
**Complexity**: 3 points
**Dependencies**: AZ-3739_sync_core_skeletons, AZ-3955_sync_core_proto
**Component**: 13_sync_core (test fixture of `sync-core`)
**Tracker**: AZ-3970
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md`, `problem_details.md`, `server_info.md`, `content_serving_headers.md` — wire shapes the fake must reproduce (drafts).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — DTOs used to script answers.

## Problem

13 is developed in parallel with the server (13 §6: "parallel with 11, 14 against a fake server"). Most protocol rules can only be tested with exact faults: a connection dropped after block 37, one flipped bit in block 7, a reset after the full body, a 429 with `Retry-After: 3`, four 401s at once. The real stack cannot produce these deterministically or quickly, and the tests.md setup names an in-process fixture with scripted drop, bit flip, delay, 401, 410, 422, 426 and 429. Every executor task (E-23/c06..c14) needs it first.

## Outcome

- A test-only fixture started in-process in under 1 s, answering the routes of `api_v1_surface.md` that 13 calls with scripted bodies in the problem-details format.
- Fault scripts: drop after N bytes or N blocks, flip one bit at an offset, delay, reset after the full body, early stream close, a status with any problem code and `Retry-After`.
- An in-memory request log (method, path, headers, body) for assertions; a second host name to catch redirected requests; TLS modes for transport tests.

## Scope

### Included
- Route handlers for server-info, nodes, revisions, lookups, uploads and blocks, staged uploads, commits and batch commits, content with ranges, changes and head, device grants and tokens, mobile code exchange, refresh, device sign-out.
- Scripting per test: ordered answers per route, fault injection, deterministic with a seed.
- TLS 1.3 with h2 by default; TLS 1.2-only and TLS 1.3 without h2 modes; a test CA trusted only by test builds.
- A redirect target host that records any request it receives.
- Recording of staged-uncommitted counts and in-flight block requests, so pacing and lane tests can assert.

### Excluded
- Real server behaviour; no test that needs server semantics (durability, restore, real conflicts) passes on the fake alone: those legs are in E-23/c18a.
- Use by host projects (14 and 17 fake `SyncApi`, not this server).

## Acceptance Criteria

**AC-1: Scripted answers in the wire format**
Given a script with a `ServerInfo`, a change page, an upload session and a problem body for each code of the `sync_api` wire table
When a client requests each route
Then the fixture answers exactly the scripted bodies, and every error body has the fields of `problem_details.md` (`type`, `title`, `status`, `code`, `request_id`, extras)

**AC-2: Faults are exact and repeatable**
Given scripts "drop after block 37", "flip bit 5 of block 7", "reset after the full body of block 3", "close the stream at 3 MiB of 10 MiB" and "delay 2 s"
When the same script runs twice with the same seed
Then each fault happens at the stated point both times and the request logs are identical

**AC-3: Transport modes**
Given the TLS 1.3 + h2, TLS 1.2-only and TLS 1.3 without h2 modes
When a client connects to each
Then the fixture negotiates exactly that mode, and the test CA is accepted only by builds compiled for tests

**AC-4: Request log and redirect catcher**
Given a script that answers one request with `302` to the catcher host
When the client runs
Then the log holds every request with its headers and body, and the catcher records whether any request (and any `Authorization` header) reached it

**AC-5: Fast and isolated**
Given 50 tests each starting their own fixture in parallel
When they run
Then each fixture starts in under 1 s on its own port and shares no state with the others

## Non-Functional Requirements

**Reliability**
- Deterministic: no wall-clock dependence; delays use the clock helper or short config values.

**Security**
- The fixture and its test CA are dev-dependencies only and never reach a release build.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Problem body shape for every code | Fields per `problem_details.md` |
| AC-2 | Fault scripts run twice | Same fault point; same log |
| AC-5 | Parallel start | < 1 s each; isolated |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-3 | Three TLS modes | Handshake | Mode as scripted; test CA in test builds only | ST-13-1 |
| AC-4 | 302 script | Catcher | Requests and headers recorded | ST-13-3 |

## Constraints

- Test-only: the fixture is a fake for the HTTP boundary of 13's own tests; integration tests always use the real server container with real PostgreSQL (tests.md isolation rule).
- Wire shapes follow the OpenAPI drafts of 11; when 11's document exists, the fixture's bodies are checked against it.

## Risks & Mitigation

**Risk 1: The fake drifts from the real server**
- *Risk*: a test passes on the fake and fails on the server.
- *Mitigation*: the real-server suite (E-23/c18a) re-runs the main cases; the fixture's bodies are validated against the OpenAPI document once it exists.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How test builds make the client trust the fixture's test CA without a release-reachable switch is not stated; decided with E-23/c07 and covered by its release feature check | E-23 | open | Low |
