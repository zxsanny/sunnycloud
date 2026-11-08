# Contract: sc_error — failure taxonomy, wire codes, status table, client parser

**Component**: shared/error (crate `sc_error`, features `server` (default) and `client`)
**Producer task**: E-02/c01 — `_docs/02_tasks/todo/B01-08_sc_error.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-00/c03, E-00/c04, E-00/c05 (`ErrorCode` in port errors), E-06/c01a, E-09/c01 (`CorrelationId`); other batches: every server component, 11 (the one mapper), 13 and every client, 09, 10
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One taxonomy (validation, expected business failure, unexpected), one stable snake_case code set and one code-to-status table, so business code never chooses a status and clients parse every error the same way. The HTTP mapper itself lives in 11 and reads this table.

Sources: `common-helpers/02_helper_error.md` (status table authoritative), `epics/E-02_cross-cutting-error-model.md`.

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `ErrorCode::as_str` | `fn as_str(self) -> &'static str` (snake_case wire code) | none | sync |
| `ErrorCode::status` | `fn status(self) -> u16` (from the single table) | none | sync |
| `BusinessError::new` | `(code: ErrorCode, params: BusinessParams) -> BusinessError` | none | sync |
| `BusinessError::not_found` | `() -> BusinessError` — the one constructor for missing ids, cross-account ids and every missing, expired or revoked share | none | sync |
| `DomainError` | `Validation(Vec<FieldProblem>) \| Business(BusinessError) \| Unexpected{source, correlation_id}` | none | |
| `CorrelationId::from_request` | `(header: Option<&str>, fresh: impl FnOnce() -> Uuid) -> CorrelationId` (UUIDv7; new when missing or malformed) | none | sync |
| `ApiError::parse` (feature `client`) | `(status: u16, headers, body: &[u8]) -> ApiError` | none (total; unknown code becomes `internal`) | sync |

| Type | Shape |
|---|---|
| `BusinessParams` | optional `need_blocks: Vec<u32>`, `retry_after: Duration`, `reason: ResyncReason \| other reason code`, `min_version` |
| `FieldProblem` | `field, problem` (no echo of secret input) |
| `ApiError` | `Api{code: ErrorCode, retry_after?, correlation_id?}` or `Transport(kind)` |
| Problem body (built by 11) | `type` (`urn:sunnycloud:error:<code>`), `title`, `status`, `code`, `request_id`, optional `retry_after`, `need_blocks`, `reason`, `min_version` |

### Status table (helper 02, row for row)

| Status | Codes |
|---|---|
| 400 | `invalid_request` |
| 401 | `unauthenticated`, `password_required` |
| 403 | `account_not_active`, `forbidden`, `csrf_rejected` |
| 404 | `parent_not_found`, `not_found`, `content_unavailable` |
| 409 | `name_conflict`, `edited_since`, `cycle`, `need_blocks`, `block_conflict`, `last_admin`, `session_cap_exceeded`, `staged_cap_reached` |
| 410 | `upload_session_gone`, `parent_deleted`, `resync_required` (with `reason`: `epoch_changed`, `below_purge_horizon`, `cursor_ahead`) |
| 411 | `length_required` |
| 413 | `size_too_large` |
| 416 | `range_not_satisfiable` |
| 422 | `invalid_name`, `invalid_cursor`, `not_a_folder`, `blocklist_mismatch`, `commit_id_reused`, `batch_too_large`, `target_not_shareable`, `block_hash_mismatch`, `block_length_mismatch` |
| 426 | `upgrade_required` |
| 429 | `rate_limited` (+ `retry_after`) |
| 500 | `internal` (tier 3 only, no detail) |
| 503 | `service_unavailable`, `overloaded` (+ `retry_after`) |
| 507 | `quota_exceeded`, `free_space_floor` |

Naming: the Rust type is `CorrelationId`; on the wire (body field, `X-Request-Id` header, logs) it is `request_id` (E-02 risk 3).

## Invariants

- Every `ErrorCode` has exactly one status; `as_str` values are unique snake_case.
- A tier-3 value never serialises its `source`: no message, path, SQL or stack trace reaches a body.
- Building a tier-2 error captures no backtrace and allocates nothing beyond the value.
- The crate has no HTTP crate dependency; statuses are plain `u16`. It builds for `wasm32-unknown-unknown` with the `client` feature.
- The workspace rejects silently discarded results: `unused_must_use` and `clippy::let_underscore_must_use` deny (set by AZ-3652) plus this crate's grep test for empty catches and `let _ =` on a `Result`.

## Non-Goals

- Not covered: the HTTP mapper, headers, OpenAPI generation (E-21); per-operation outcome enums (component epics).

## Versioning Rules

- A new code is a minor change and needs its status row in the same change; changing a code's status or spelling is breaking (clients and OpenAPI depend on it).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| one-status | enumerate all codes | 0 duplicates, 0 unmapped | E-02 AC 1 |
| quota-507 | `quota_exceeded`, `free_space_floor` | 507 | AC-39 |
| not-found-bytes | body from `not_found()` for a missing, an expired and a revoked link | byte-identical | AC-12 |
| roundtrip | `BusinessError` with `need_blocks` -> body -> `ApiError::parse` | same code and params | |
| unknown-code | body with code `brand_new` | `internal`, request id kept | |
| tier3-opaque | `Unexpected` with a planted secret in `source` | body has no secret | NFT-SEC-38 step 3 |

## Open points (flagged in the producer task)

1. Codes used by other specs but absent from helper 02's table: 08 `lease_lost` (409), `revision_obsolete` (404), `thumbnail_not_ready`, `thumbnail_unavailable` (404), output too large (413) and type or kind rejected (422) on the worker routes; 20 `PreconditionFailed` (412) and a missing `If-Match` (428); 01 RFC 8628 device-poll codes (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`). E-02 implements the table as written; each owning epic adds its rows (minor version).
2. Helper 02 shows the body field as `correlation_id`; E-02 fixes the wire name as `request_id`. Helper 02 needs a doc fix.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
