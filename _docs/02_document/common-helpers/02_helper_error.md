# Helper: Error (typed results and the single mapping boundary)

**Crate**: `sc_error`. **Used by**: all server components (types), 11_http_api (the mapping boundary), 13_sync_core and every client (wire parsing). Generated TypeScript types for 16_web_app come from OpenAPI, not from this crate.

## Purpose
One taxonomy of failures, one stable wire vocabulary, one place where an error becomes an HTTP response. Business components return typed results; nothing below the delivery layer knows a status code.

## Three tiers

> Naming: the correlation id is called `correlation_id` in Rust types and `request_id` on the wire (JSON body, `X-Request-Id` header, logs); the HTTP mapper in 11 does the renaming. The status table is plain `u16` data in `sc_error`.

| Tier | Meaning | Representation | Handling |
|------|---------|----------------|----------|
| 1 Validation | Bad input from the caller (names, ranges, cursors) | `Validation(Vec<FieldProblem>)` returned as a value | 400/422 at the boundary, never thrown |
| 2 Expected business failure | Defined outcomes: `name_conflict`, `edited_since`, `quota_exceeded`, `need_blocks`, `blocklist_mismatch`, `resync_required`, `password_required`, `not_found` | Per-operation `enum XxxOutcome` (typed result) built on the shared `BusinessError` code set | Mapped to 404/409/422/403/426/429 by the mapper |
| 3 Unexpected | Bugs, I/O faults, database down | `Unexpected { source, correlation_id }` | One central handler: opaque 500/503 to the client, full error and correlation id in the log |

No `catch`-all: a catch may only (a) convert a third-party error into a tier 2 or 3 value, (b) rethrow, (c) log-and-continue in an explicitly best-effort path named in that component's spec. Hot per-item paths (block PUT, feed rows, listing rows) return values, they do not throw.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `ErrorCode` (enum) | none | stable snake_case wire code | none |
| `BusinessError::new` | `ErrorCode`, optional typed params (`need_blocks`, `retry_after`, `reason`) | `BusinessError` | none |
| `ErrorMapper::to_response` (in 11_http_api only) | `&DomainError`, `CorrelationId`, request context | `ProblemResponse { status, headers, body }` (RFC 9457 `application/problem+json`: `code`, `title`, `correlation_id`, optional `retry_after`, `need_blocks`, `reason`) | none |
| `CorrelationId::from_request` | optional `X-Request-Id` header | UUIDv7 (new when missing or malformed) | none |
| `ApiError::parse` (clients) | status, headers, body | `ApiError { code: ErrorCode, retry_after, correlation_id }` or `ApiError::Transport(kind)` | none |

## Status mapping (single table, owned here)

| Code | HTTP | Notes |
|------|------|-------|
| `invalid_request` | 400 | field problems listed |
| `invalid_name`, `invalid_cursor`, `not_a_folder`, `blocklist_mismatch`, `commit_id_reused`, `batch_too_large` | 422 | `invalid_cursor`: malformed or foreign cursor on the listing and changes operations; `commit_id_reused`: same commit id, different request hash; `batch_too_large`: more entries than `batch_max_entries` (or more than 1000 hashes in a lookup) |
| `unauthenticated` | 401 | token revoked or missing |
| `account_not_active` | 403 | account disabled or deleting at start, put or commit (03, 02) |
| `forbidden` | 403 | device token on privileged route, CSRF failure (no `Origin` and no `Sec-Fetch-Site`), worker route with a user token (AC-79) |
| `parent_not_found` | 404 | upload or commit names a parent id that does not exist |
| `not_found` | 404 | also every cross-account ID (AC-59) and every missing/expired/revoked share (identical body, headers; AC-12) |
| `name_conflict`, `edited_since`, `cycle`, `need_blocks`, `block_conflict`, `last_admin`, `session_cap_exceeded`, `staged_cap_reached` | 409 | client has a defined recovery (`name_conflict` never occurs on a commit: new files get a conflict-copy name); the two caps clear when sessions finish or are aborted, or staged files are committed |
| `upload_session_gone`, `parent_deleted` | 410 | client restarts the upload or recreates the parent |
| `password_required` | 401 | share link with a password; body `{code: "password_required"}` |
| `content_unavailable` | 404 (distinct code) | blob missing or suspect; never 5xx (AC-73) |
| `range_not_satisfiable` | 416 | |
| `target_not_shareable` | 422 | share target is a folder or tombstoned (v1.0 shares files only) |
| `block_hash_mismatch`, `block_length_mismatch` | 422 | block not stored |
| `length_required` | 411 | block PUT without `Content-Length` |
| `size_too_large` | 413 | declared size above `max_declared_size` |
| `quota_exceeded`, `free_space_floor` | 507 | typed, nothing reserved; `quota_exceeded` is fixed at 507 (owner decision) |
| `rate_limited` | 429 + `Retry-After` | AC-20 |
| `upgrade_required` | 426 | stable code, update location (AC-64) |
| `resync_required` | 410 on the changes operation (data model 7.2) | extra field `reason`: `epoch_changed`, `below_purge_horizon` or `cursor_ahead` |
| `service_unavailable`, `overloaded` | 503 + `Retry-After` | database down, or the buffer, lane or in-flight cap is full (03); clients back off |
| `internal` | 500 | tier 3 only; no detail |

## Rules
- Error bodies for tier 3 never include messages, paths, SQL or stack traces. Tier 1/2 bodies never echo secrets or other accounts' data.
- Share-link and cross-account failures go through the same `not_found` constructor so the response bytes are identical; the mapper has no branch that tells them apart.
- Clients treat unknown codes as `internal` and keep the correlation id for the user-visible "report this" text.
- The mapping table is the only place status codes are chosen; the OpenAPI document is generated from it.

## Tests
FT-N-27 (426 upgrade message), FT-N-09 and NFT-SEC-17 (identical not-found), NFT-SEC-13 (404 matrix), NFT-SEC-09 (403 CSRF), NFT-SEC-04/05/06 (429 with Retry-After), FT-N-02 (corruption never delivered as a good file), unit tests for the mapping table (every `ErrorCode` has exactly one status).

## Risks
- Quota failures have one status (507) and one code; the OpenAPI document is generated from this table, so clients cannot see another.
- Silent catch regressions: a lint (`clippy::let_underscore_must_use`, grep test for empty `catch`/`let _ =` on `Result`) is a CI gate.
