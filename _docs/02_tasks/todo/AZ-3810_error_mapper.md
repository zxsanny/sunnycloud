# Error mapper to `application/problem+json` with typed extras

**Task**: AZ-3810_error_mapper
**Name**: Error mapper
**Description**: The one place that turns a `DomainError` into an HTTP status, headers and a problem body, using the `sc_error` status table, with opaque unexpected errors logged once and `Retry-After` on 429 and 503
**Complexity**: 3 points
**Dependencies**: AZ-3800_status_assignment_spike, AZ-3801_crate_skeleton_accept_loop, AZ-3764_sc_error
**Component**: 11 http_api
**Tracker**: AZ-3810
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_error/sc_error.md` — codes, tiers, status table, `CorrelationId` (E-02/c01).
- `_docs/02_document/contracts/11_http_api/http_server.md` — the server this mapper runs in (E-21/c03).

## Problem

Business components return typed errors and never choose a status (helper 02). Clients parse every failure with one parser, keyed on `code`, and back off on `Retry-After` (AC-20). Unexpected failures must never leak SQL, paths or keys to the client, and must be findable in the log by the request id the user reports.

## Outcome

- Every error answer of the public and internal listeners has the problem body of `problem_details.md`, with the status taken from the `sc_error` table.
- 429 and 503 carry `Retry-After` and `retry_after`; `need_blocks`, `reason` and `min_version` extras appear with their codes.
- Unexpected errors give one opaque 500 body and exactly one ERROR log line with the full chain and the request id.

## Scope

### Included
- Mapping of the three tiers (validation, business, unexpected) to status, headers and body; typed extras.
- The per-port error conversion used by handlers: each port error variant of `contracts` parts 1-3 to its `sc_error` code, so handlers stay "bind, call, map".
- Problem schema in the OpenAPI components (consumed by E-21/c11).
- The statuses decided by E-21/c02 for `need_blocks`, `edited_since` and batch-too-large.

### Excluded
- The code list and status table (E-02); request ID generation (E-21/c04); body-limit and version-gate statuses are produced by E-21/c05 and E-21/c06 through this mapper.
- Stream errors after headers (E-21/c10).

## Acceptance Criteria

**AC-1: Status table (AC-20, AC-64)**
Given one stub route per tier and per code family: validation (400 and a semantic 422), `unauthenticated`, `forbidden`, `not_found`, `name_conflict`, `block_conflict`, `rate_limited` with retry_after 17, `resync_required`, `upload_session_gone` (expired upload session), `need_blocks`, `edited_since`, `size_too_large`, `range_not_satisfiable`, `upgrade_required`, `service_unavailable`, `quota_exceeded`, `last_admin` and an unexpected error
When each is called
Then the statuses are 400/422, 401, 403, 404, 409, 409, 429, 410, 410, the status E-21/c02 recorded for `need_blocks` and for `edited_since`, 413, 416, 426, 503, 507, 409 and 500, each with a body whose `type` is `urn:sunnycloud:error:<code>`, `status` equals the HTTP status and `request_id` equals the `X-Request-Id` header

**AC-2: Retry-After and extras (AC-20)**
Given `RateLimited{retry_after: 17}`, `service_unavailable` with a retry_after, `need_blocks [3, 7]` and `resync_required` with reason `epoch_changed`
When each is mapped
Then 429 and 503 carry `Retry-After` equal to the body's `retry_after`, the `need_blocks` body lists 3 and 7, and the 410 body carries `reason: epoch_changed`

**AC-3: Unexpected errors are opaque (AC-69 response part)**
Given a stub that returns an unexpected error whose chain contains `SELECT * FROM nodes WHERE ...`, `/var/lib/sunnycloud/blobs/ab/cd/x` and the marker `K3Y-MARK`
When the route is called
Then the 500 body contains none of the three, and the log holds exactly one ERROR line with the full chain and the same `request_id`

**AC-4: Every port error has a code**
Given the error enums of every port in `ApiServices`
When the conversion table is checked
Then every variant maps to exactly one `sc_error` code (none falls through to `internal` except the tier-3 `Unexpected`), and a new variant without a mapping fails the build or the test

**AC-5: Uniform not-found (AC-59, AC-12)**
Given a foreign id and a random id on the same route
When both are mapped
Then the two 404 bodies are identical except for `request_id`

## Non-Functional Requirements

**Reliability**
- Mapping adds no await and no I/O between the port's return and the response.

**Security**
- No `detail` text is built from request data, SQL, paths, keys, tokens, names or share URLs.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Tier and code to status, every code of `sc_error` | One status per code, equal to the table |
| AC-4 | Exhaustive conversion of every port error variant | No unmapped variant |
| AC-5 | Bodies for foreign and random ids | Equal apart from `request_id` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, one stub per kind | Error table (IT-11-1) | Statuses and body fields as listed | AC-20, AC-64 |
| AC-2 | Mode S | Rate-limit and unavailable mapping (IT-11-15) | `Retry-After` header equals body | AC-20, NFT-SEC-04 |
| AC-3 | Mode S, planted chain | Opaque internal errors (IT-11-2, opacity part) | Nothing leaked; one ERROR line | NFT-SEC-38 |
| AC-5 | Mode S | Foreign versus random id | Identical bodies | NFT-SEC-13, NFT-SEC-17 |

## Constraints

- The status comes only from the `sc_error` table; this crate keeps no second table.
- The wire field is `request_id` (E-02 decision), even where helper 02 still shows `correlation_id`.

## Risks & Mitigation

**Risk 1: Codes missing from the table**
- *Risk*: owners use codes helper 02 does not list (`lease_lost`, `revision_obsolete`, thumbnail codes, RFC 8628 poll codes).
- *Mitigation*: AC-4 fails the build until the owning epic adds the `sc_error` row.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-11-1 statuses for `need_blocks`/`edited_since` come from E-21/c02 | E-13, E-14, E-21 owners | open | Medium |
| Batch too large 413 vs 422 (F5, FT-N-48 vs helper 02, 03) | E-14, E-02 owners | open | Medium |
| Codes missing from helper 02's table: 08 `lease_lost`, `revision_obsolete`, `thumbnail_not_ready`, `thumbnail_unavailable`, output too large, type or kind rejected; 01 RFC 8628 poll codes; the long-poll hold-cap error (G-05-2) | E-18, E-15, E-12 owners with E-02 | open | Medium |
| `Retry-After` value on a 503 whose error carries no `retry_after` is not specified | E-02, E-21 owners | open | Low |
| 11 describes `AppError{kind, code, detail, extra}` with an `ErrorKind`; `sc_error` defines `DomainError` with `ErrorCode`. This task maps `sc_error`'s shape | E-02, E-21 owners | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/11_http_api/problem_details.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
