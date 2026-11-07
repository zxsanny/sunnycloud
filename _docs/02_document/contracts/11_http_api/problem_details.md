# Contract: problem_details — `/api/v1` error responses (`application/problem+json`)

**Component**: 11_http_api
**Producer task**: E-21/c09 — `_docs/02_tasks/todo/B07-09_error_mapper.md` (statuses for `need_blocks`/`edited_since`/`batch_too_large` decided 2026-11-07, spike E-21/c02); keys assigned at Step 4
**Consumer tasks**: E-23/c06 (typed error model and wire-code mapping), E-23/c07 (426 handling), E-26/c06 (SPA error mapping), E-20/c03 (`WorkerApi` status mapping), E-24 and E-27 through `sync-core`/`mobile_backup`, E-31 consumer framework; every E-21 handler task (c04..c17) produces errors through it; resolved at Step 4
**Version**: 1.0.0
**Status**: draft (G-11-1 and batch-too-large decided 2026-11-07)
**Last Updated**: 2026-11-04

## Purpose

One error shape for every non-2xx answer of the public and internal listeners, so clients parse every failure the same way (`sc_error::ApiError::parse`). The status of each code comes from the single table in `sc_error` (`../shared_error/sc_error.md`); 11 is the only place that turns a `DomainError` into HTTP and never picks a status of its own beyond that table.

Sources: `components/11_http_api/description.md` §6; `common-helpers/02_helper_error.md`; `contracts/shared_error/sc_error.md`; component specs 01, 02, 03, 05, 06, 07, 08 (their error rows).

## Shape

### Body (`Content-Type: application/problem+json`)

| Field | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| `type` | string | yes | `urn:sunnycloud:error:<code>` | |
| `title` | string | yes | short constant text per code | never contains request data |
| `status` | integer | yes | equal to the HTTP status | |
| `code` | string | yes | stable snake_case code from `sc_error` | unknown codes are parsed as `internal` by clients |
| `detail` | string | no | safe text only | never SQL, paths, keys, tokens, names or share URLs |
| `request_id` | string | yes | UUIDv7, equal to the `X-Request-Id` header | |
| `retry_after` | integer (seconds) | with 429 and 503 | equal to the `Retry-After` header | |
| `need_blocks` | array of integers | with `need_blocks` | missing block indexes | |
| `reason` | string | with `resync_required` | `epoch_changed`, `below_purge_horizon`, `cursor_ahead` | |
| `min_version`, `update_url` | string | with `upgrade_required` | see `server_info.md` | |

### Headers on every error

`X-Request-Id`, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, the security headers of `api_v1_surface.md`; `Retry-After` (seconds) on 429 and 503.

### Status by tier

| Tier (`sc_error`) | Status |
|-------------------|--------|
| Validation (`invalid_request`, field problems) | 400 |
| Business (`BusinessError` with a code) | the code's row in the `sc_error` status table (400, 401, 403, 404, 409, 410, 413, 416, 422, 426, 429, 503, 507) |
| Unexpected | 500 `internal`, opaque body (no `detail`); logged once at ERROR with the full chain and the request id |

### Rules fixed by component specs (mapped here, decided by the owners)

| Case | Answer | Owner |
|------|--------|-------|
| missing, foreign, expired or revoked id or share | 404 `not_found`, byte-identical apart from `request_id` | 02, 03, 06 (AC-12, AC-59) |
| missing blob file | 404 `content_unavailable`, never 5xx | 03, 04 (AC-73) |
| quota exceeded | 507 `quota_exceeded`, nothing reserved | 07a |
| last admin | 409 `last_admin` | 07 (G-11-5) |
| password-protected share without grant | 401 `password_required` | 06 (G-9) |
| non-admin on an admin route | 403 `forbidden` | 07, 11 (G-S6) |
| `resync_required` | 410 with `reason` | 05 |
| block PUT without `Content-Length` | 411 | 11 |
| declared body over the route class (including a `POST /commits:batch` body over the 4 MiB `Batch` class) | 413 `size_too_large` | 11 |
| `need_blocks`, `edited_since` | 409 (412 is reserved for `If-Match` header preconditions) | 03, 02 (G-11-1) |
| more entries than `batch_max_entries` | 422 `batch_too_large`, produced by the commit service, not by this layer | 03 |
| client below the minimum version | 426 `upgrade_required` | 11 |

## Invariants

- Every error answer has the body above and its `status` equals the HTTP status.
- `request_id` in the body, the `X-Request-Id` header and the log line are one value.
- A tier-3 error never reveals its source; the client sees the same opaque body for every unexpected failure.
- An error after response headers were sent aborts the stream (RST_STREAM on HTTP/2, connection close on HTTP/1.1); it never ends as a clean shorter body.
- Business code never chooses a status; adding a code means adding its `sc_error` row first.

## Non-Goals

- Not covered: the code list and status table themselves (`sc_error`, E-02); per-operation outcome enums (component contracts).
- Not covered: the RFC 8628 device-poll answers (Open point 3).

## Versioning Rules

- **Breaking**: a field removed or renamed, a code's status changed, `type` format changed.
- **Non-breaking**: a new code with its status row, a new optional extra field.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| per-kind | one stub error per tier and code family | status per table; body fields as above | IT-11-1 |
| rate-limited | `RateLimited{retry_after: 17}` | 429, `Retry-After: 17`, `retry_after: 17` | IT-11-15, AC-20 |
| opaque-500 | `Unexpected` with SQL, a path and a marker in the chain | 500 without them; one ERROR line with the chain and the same `request_id` | IT-11-2 |
| quota | `quota_exceeded` | 507 | AC-39 |
| uniform-404 | foreign id versus random id | identical bodies apart from `request_id` | NFT-SEC-13 |
| stream-abort | stream error after 3 MiB of 10 MiB | h2 RST_STREAM; h1 close before `Content-Length` | IT-11-20 |

## Open points

1. G-11-1: closed 2026-11-07 (owner decision): `need_blocks` and `edited_since` are both 409, as in helper 02, 02 and 03; 412 is for `If-Match` header preconditions, which these are not.
2. Batch too large: closed 2026-11-07 (owner decision): `batch_too_large` is 422 for more entries than `batch_max_entries`; a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer (`size_too_large` family). F5, FT-N-48 and pipeline AZ-3656 follow (413 became 422 for too many entries).
3. Codes used by owners but missing from helper 02's table: 08 `lease_lost` (409), `revision_obsolete` (404), `thumbnail_not_ready`, `thumbnail_unavailable` (404), output too large (413), type or kind rejected (422); 01 RFC 8628 poll codes (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`) whose status and body shape are not stated; 20's 412 and 428 (v1.2). Each owning epic adds its rows (`sc_error` minor version).
4. `Retry-After` value on a 503 whose error carries no `retry_after` is not specified.
5. Helper 02 still shows the body field as `correlation_id`; this contract uses `request_id` (E-02 decision).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
| 1.0.1 | 2026-11-07 | Closed open points 1 and 2: `need_blocks`, `edited_since` 409; `batch_too_large` 422, body over `Batch` class 413 | AZ-3800 |
