# Timeout and body limits per `BodyClass` (411, 413, idle, long-poll)

**Task**: AZ-3803_body_limits_timeouts
**Name**: Body classes and timeouts
**Description**: Enforce each route's body class before reading the body (413 on a declared oversize, 411 for a block PUT without `Content-Length`, abort of chunked oversize) and the time limits per route class (body idle, long-poll wait plus 5 s, download write-idle, default request timeout)
**Complexity**: 3 points
**Dependencies**: AZ-3801_crate_skeleton_accept_loop, AZ-3810_error_mapper, AZ-3827_route_registry_openapi
**Component**: 11 http_api
**Tracker**: AZ-3803
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — body classes, limits and per-route classes (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — 411 and 413 bodies (E-21/c09).

## Problem

Large HTTP/2 windows and 4 MiB commit bodies must not turn into buffered memory for a peer that sends too much or too slowly (AC-59, R13). Uploads need exact block lengths, long-polls need to outlive the default timeout, and downloads of large files must not be cut by a total time cap while still ending a stalled reader.

## Outcome

- A request whose declared length exceeds its route's class limit gets 413 before any body byte is read, counted in `body_limit_exceeded_total`.
- A block PUT without `Content-Length` gets 411; a chunked body is cut at the limit.
- Time limits apply per class: body idle (`body_idle_timeout`) for `Block` and `Staged`, `wait` + 5 s for long-polls, 60 s write-idle with no total cap for downloads, `request_timeout` otherwise.

## Scope

### Included
- Limits per class: `Small` 64 KiB, `Commit` 4 MiB, `Batch` 4 MiB, `Block` at most 2 MiB with `Content-Length` required, `Staged` at most 2 MiB (0 allowed), `Query` 128 KiB, `Auth` 16 KiB, `WorkerOutput` the per-kind cap given by 08's configuration, `None` for GET and HEAD.
- Declared-length check before reading, streaming cut for chunked bodies, the idle-body timer, the per-class request timer, the download write-idle timer.
- Exact block length per index is checked by 03 (`LengthMismatch`); this layer enforces only the 2 MiB ceiling and the header.

### Excluded
- Auth before body (E-21/c07 orders auth ahead of this read); the hold logic of long-polls (05).
- Choosing the per-kind `WorkerOutput` caps (08 config).

## Acceptance Criteria

**AC-1: Declared oversize refused before reading (AC-59)**
Given routes of class `Small`, `Commit`, `Auth`, `Query` and `Block`, and a headers-only request declaring 10 GiB
When each receives a declared length one byte over its limit, and the 10 GiB request arrives
Then each answers 413 within 100 ms with 0 body bytes consumed, and `body_limit_exceeded_total` rises once per request

**AC-2: Block PUT needs a length**
Given a block route
When a PUT arrives without `Content-Length`
Then the answer is 411 and the stub port is not called

**AC-3: Chunked oversize is cut**
Given a `Small` route
When a chunked body without a declared length passes 64 KiB
Then reading stops at the limit and the request ends with 413 or a reset, and the stub port is not called

**AC-4: Slow body and request timeout**
Given `body_idle_timeout = 1s` and `request_timeout = 2s`
When a `Block` body arrives at 1 byte per 500 ms after a 1.5 s pause, and a stub handler takes 3 s
Then the slow body ends at the idle timeout (about 1 s) with 408 or a reset, and the slow handler's request ends at 2 s

**AC-5: Long-poll and downloads keep their own clocks**
Given `request_timeout = 2s`, a long-poll with `wait = 4s` whose stub answers at 4 s, a second long-poll whose stub never answers, and a download stub that streams for 10 s without pauses
When they run
Then the first long-poll is answered at 4 s, the second ends at `wait` + 5 s, and the download completes; a download whose reader stops reading is closed after the write-idle timeout

**AC-6: GET with a body is not buffered**
Given a `None` class GET route
When a request with a 1 MiB body arrives
Then the body is rejected or ignored without being buffered, and server memory does not grow by the body size

## Non-Functional Requirements

**Performance**
- Limit checks add no copy of the body; a `Block` body streams to the port.

**Reliability**
- Every duration is a config value; tests use short ones.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Class to limit table, including `WorkerOutput` per kind | One limit per class; boundary values pass, plus one fail |
| AC-5 | Timer choice per route class | Long-poll `wait` + 5 s; downloads write-idle only |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, one route per class | Declared oversize and 10 GiB headers (IT-11-13) | 413 within 100 ms, 0 bytes read, metric +1 | AC-59, NFT-SEC-15 |
| AC-2 | Mode S | Block PUT without length (IT-11-13) | 411 | FT-P-05 |
| AC-3 | Mode S | Chunked oversize (IT-11-13) | Cut at limit | |
| AC-4 | Mode S, short timers | Slow body, slow handler (IT-11-13) | Idle and request timeouts apply | NFT-SEC-15 |
| AC-5 | Mode S | Long-poll and download timers (IT-11-13) | Own clocks honoured | AC-1 |
| AC-6 | Mode S | GET with body (IT-11-13) | Not buffered | |

## Constraints

- The class of each route comes from the registry; no handler sets its own limit.

## Risks & Mitigation

**Risk 1: Batch limit and entry count disagree**
- *Risk*: a 4 MiB body may hold more than 500 entries or fewer; the byte limit and the entry limit give different statuses.
- *Mitigation*: the byte limit is enforced here; the entry count is 03's (`BatchTooLarge`), with its status from E-21/c02.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Body class of `POST /api/v1/revisions:lookup` (1,000 hashes, about 70 KB) is not assigned; `Small` is too small | E-13, E-21 owners | open | Medium |
| Status of a slow body at the idle timeout is "408 or reset" in IT-11-13, not fixed | E-21 owner | open | Low |

## Contract

This task implements the body-class and time-limit rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
