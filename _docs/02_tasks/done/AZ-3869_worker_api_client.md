# WorkerApi HTTP client: status mapping and hash-verified streaming input

**Task**: AZ-3869_worker_api_client
**Name**: WorkerApi client
**Description**: The supervisor's HTTP client of the worker listener: claim, heartbeat, release, fetch input, post output, complete, fail and status, each mapped from HTTP status to a typed result, with the input streamed, size-checked and verified by BLAKE3 block hashes against the claimed content hash
**Complexity**: 3 points
**Dependencies**: AZ-3868_worker_crate_skeleton, AZ-3766_sc_ids, AZ-3771_media_shared_skeleton, AZ-3837_media_shared_params_protocol
**Component**: 09_worker_supervisor
**Tracker**: AZ-3869
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `ClaimedJob`, `Lane`, `DerivedKind`, `OutputMeta`, `SkipOutcome`, `FailureReport`, `FailResult`, `WorkerStatus`, produced by E-00/c06.
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `hash_block`, `rollup`, `BLOCK_SIZE`, produced by E-04/c01.
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — routes, statuses and per-version values of the worker listener, produced by E-18/c02.

## Problem

Every job state change goes through the API worker listener (S4-1). The supervisor must react to each status exactly as 09 §2 states (404 closes `obsolete`, 409 stops quietly, 403 stops claiming), and the bytes it hands to the decoder must be the stored plaintext: a wrong master key or a wrong revision must never reach a decoder (AC-72, R5).

## Outcome

- One typed result per call; no caller inspects HTTP statuses.
- The input lands in its destination only when its BLAKE3 rollup equals `ClaimedJob.content_hash`; size limits hold before and during streaming.
- The bearer and `X-Worker-Id` are on every request and never logged.

## Scope

### Included
- `claim(lane)`: 200 -> `ClaimedJob`, 204 -> none; `heartbeat`: `Extended` or `LeaseLost` (409); `release`; `complete(skipped_compliant | obsolete)`; `fail(code, Retry | Permanent)` -> `Requeued | Dead`; `report_status({decoder_ok, image_age_days})`.
- `fetch_input`: `Content-Length` checked against `worker_max_input_photo` (stills) or `worker_max_input_video` (poster, transcode) before the first byte is written; the limit enforced while streaming; BLAKE3 per 2 MiB block computed on the fly and rolled up with the helper; mismatch -> `HashMismatch`; 404 `Gone`, 409 `LeaseLost`, 403 `Forbidden`.
- `post_output(kind)`: body streamed from the file handle given by `Scratch`, with `Content-Type`, `X-Width`, `X-Height`, `X-Duration-Ms`; 204 accepted and 200 already present both success; 409, 404, 413 `TooLarge`, 422 `TypeRejected`.
- Connection errors and 5xx -> `Transient` on every call.
- Bearer and `X-Worker-Id` on every request; the token is never in logs, metrics or errors.

### Excluded
- What the loop does with each result (E-20/c06, c07); creating the destination file and its mode (E-20/c04); the server side (E-18, E-21).

## Acceptance Criteria

**AC-1: Status mapping**
Given a listener answering each documented status in turn
When each call is made
Then claim gives none for 204 and a job for 200; heartbeat gives `LeaseLost` for 409; fetch gives `Gone` (404), `LeaseLost` (409), `Forbidden` (403); post gives success for 204 and 200, `TooLarge` for 413, `TypeRejected` for 422; fail returns `Requeued` or `Dead`; any call gives `Transient` for 503 or a refused connection

**AC-2: AC-72 worker side: verified input (R5)**
Given a job whose `content_hash` does not match the served bytes, and one whose hash matches
When each input is fetched
Then the first returns `HashMismatch` and its destination holds no usable input, and the second's destination equals the served bytes with the BLAKE3 per 2 MiB block rolled up equal to `content_hash`

**AC-3: Size limits (IT-09-7)**
Given `worker_max_input_photo = 1MiB`
When a 2 MiB photo is fetched, and a body grows past its `Content-Length`
Then the first returns `TooLarge` before any byte is written (destination size 0), and the second stream is aborted at the limit

**AC-4: Credentials (AC-79, ST-09-1)**
Given a 48-character marker as the bearer
When every call runs, including failing ones
Then every request carries the bearer and `X-Worker-Id`, and the marker appears in no log line, metric or error value

**AC-5: Output headers**
Given a 1440 px WebP and a transcode with width, height and duration
When each is posted
Then `Content-Type`, `X-Width`, `X-Height` and (for video) `X-Duration-Ms` carry the values passed in, and the body equals the file

## Non-Functional Requirements

**Performance**
- Streaming peak RSS for a 1 GiB input target 64 MiB, failure 128 MiB (PT-09-3, measured in E-20/c12).

**Reliability**
- A hash mismatch never reaches the decoder (AC-72).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Status-to-result mapping against a local HTTP peer per status | Typed result per row |
| AC-2 | Block hashing and rollup against the `sc_ids` vectors (0 B, 1 B, 2 MiB, 2 MiB + 1 B) | Equal hashes |
| AC-3 | Declared and actual length cases | Refused early; aborted at limit |
| AC-4 | Log and error output with a marker token | Marker absent |
| AC-5 | Output request headers and body against a local HTTP peer | Headers carry the passed values; body equals the file |

## Blackbox Tests

Run against the real worker listener in harness H2 (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2, AC-3 | Edited job hash; small input limit; growing body | IT-09-7 (a)-(d) | `HashMismatch`; `TooLarge`; abort; exact source | AC-72, FT-N-43 |
| AC-1 | Deleted revision | IT-09-8 | `Gone` | |
| AC-1, AC-4 | Lease expired; token replaced | IT-09-10 | `LeaseLost`; `Forbidden`; no token in logs | AC-79, NFT-SEC-50 |

## Runtime Completeness

- Named capability: HTTP client of the API worker listener `:8081` (plaintext on the backend network).
- Unacceptable substitutes: any database access or a second data path for input; the supervisor has no database login.

## Constraints

- The client measures only local durations; all lease and backoff time is the API's (09 §2).

## Risks & Mitigation

**Risk 1: Buffering a whole input in memory**
- *Risk*: a 10 GiB video read into memory exhausts the 512 MiB worker limit.
- *Mitigation*: streaming with per-block hashing; PT-09-3 bounds RSS.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The input route always sends an exact `Content-Length` (`worker_job_protocol.md`); the `content_hash` JSON encoding is still open there (open point 1, E-18/c01) | E-18 owner | open | Low |
