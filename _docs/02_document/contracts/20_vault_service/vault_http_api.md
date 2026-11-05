# Contract: vault_http_api — `/api/v1/vaults` routes [v1.2]

**Component**: 20_vault_service (routes registered in 11's registry, served by 11, wired by 12)
**Producer task**: E-30/c06 — `_docs/02_tasks/todo/B11-21_vault_routes_registration.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-30/c08 (integration and security suite), E-30/p01 (`AZ-3675_pipeline_vault`), the client vault pages of 16 (E-26) and 17/18 (E-27, E-28) that call these routes with header bytes produced by `vault-core` (19) — those client tasks are not planned yet (flagged on AZ-3675)
**Version**: 1.0.0
**Status**: draft (20 is a stub-level proposal for the external review, 20 §1; frozen when E-30/c06 is accepted and the open points are closed)
**Last Updated**: 2026-11-04

## Purpose

The four HTTP operations through which a client creates a vault, reads and replaces its opaque header with optimistic concurrency, and reports an unlock failure. Clients (16, 17, 18) and the vault core's callers depend on the exact status and header rules, so they read this file, not the producer task. Sources: `components/20_vault_service/description.md` §3, §4, §7; `epics/E-30_vault_service.md`; `components/20_vault_service/tests.md`; `contracts/11_http_api/api_v1_surface.md`.

## Shape

### HTTP endpoints

| Method | Path | Auth class | Mut | Port method | Request | Success response | Error statuses |
|--------|------|-----------|-----|-------------|---------|------------------|----------------|
| `POST` | `/api/v1/vaults` | `SessionOnly` | yes | `VaultService::create_vault` | header bytes and `root_node_id` (encoding: Open point 1) | 201; `ETag` response header; body with the vault id | 401, 403 (device token; CSRF), 404 (root unknown, foreign or tombstoned), header too large (Open point 3), root is a file and second vault on a root (Open point 3), `VaultExists` if decided (Open point 3) |
| `GET` | `/api/v1/vaults/{id}` | `Session` | no | `VaultService::get_header` | none | 200; header bytes, root node id; `ETag` response header | 401, 403, 404 |
| `PUT` | `/api/v1/vaults/{id}/header` | `SessionOnly` | yes | `VaultService::replace_header` | header bytes; `If-Match` request header required | 200; new `ETag` response header | 401, 403, 404, 412 (`If-Match` differs from the stored ETag; stored header unchanged), 428 (no `If-Match`; stored header unchanged), header too large (Open point 3) |
| `POST` | `/api/v1/vaults/{id}/unlock-failures` | `Session` | yes | `VaultService::report_unlock_failure` | no fields; any body content is ignored and never stored or logged | success status (Open point 4) | 401, 403, 404 (unknown or foreign id; nothing written) |

`x-id-params`: `{id}` (path) on the last three operations; `root_node_id` (body) on create.

### Headers

| Header | Direction | Rule |
|--------|-----------|------|
| `ETag` | response of create, GET and PUT | derived from the SHA-256 of the stored header bytes; equal on every read of unchanged bytes; string form: Open point 2 |
| `If-Match` | request of PUT | required; compared with the ETag of the stored bytes at write time |

### Field table (request and response values)

| Field | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| header | bytes | yes (create, PUT; GET response) | opaque authenticated vault header from `vault-core` | 1 to `max_vault_header_bytes` bytes; never parsed or altered by the server |
| `root_node_id` | UUID | yes (create; GET response) | the vault's root folder | a live folder of the caller |
| vault id | UUID | yes (create response; path elsewhere) | identifies the vault | |

## Invariants

- The header returned by GET is byte-identical to the last stored header (AC-61, IT-20-1).
- The `ETag` is derived from the SHA-256 of the stored bytes, so equal bytes give equal ETags across reads and restarts.
- A PUT changes the stored header only when `If-Match` equals the current ETag; among concurrent PUTs with the same `If-Match` exactly one succeeds and the rest get 412 (IT-20-2).
- A missing `If-Match` never changes the stored header (428).
- Every operation is account-scoped: another account's vault id gets 404, the same body as an unknown id apart from `request_id` (AC-59, ST-20-1).
- A device token gets 403 on the two `SessionOnly` operations (ST-20-1 step 2); cookie-authenticated POST and PUT follow the CSRF rule of `api_v1_surface.md` (AC-58).
- No response, error body or log line carries a passphrase, a key, plaintext or the body of an unlock-failure report (20 §7, ST-20-2).
- The routes exist only in v1.2 builds; v1.0 and v1.1 builds expose no vault operation (FT-P-73 step 2, ADR-016).
- Errors use `problem_details.md`; the codes and statuses are rows of the `sc_error` table.

## Non-Goals

- Not covered: the header byte layout, the generation counter and rollback detection (19, clients).
- Not covered: vault file content and wrapped file keys; they travel through the normal upload, commit and download routes and `revisions.client_meta`.
- Not covered: share links for vault files (refused by 06 with 422 `target_not_shareable`).

## Versioning Rules

- **Breaking changes** (route, method, auth class or required header changed; a field removed; a status changed) require a new major version and a deprecation path; after v1.2 ships the E-21 additive-only diff gate applies.
- **Non-breaking additions** (a new optional response field, a new error code clients already treat as generic) require a minor bump.
- While `Status` is draft (before E-30/c06 is accepted) the shape may change without a major bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| round-trip | create with 1-byte, 512-byte and 256-value headers; GET twice and after restart | same bytes; same ETag on each read | IT-20-1 |
| missing-if-match | PUT without `If-Match` | 428; stored header unchanged | IT-20-2 |
| stale-if-match | PUT with an old ETag | 412; stored header byte-equal to before | IT-20-2 |
| race | 50 concurrent PUTs with the same current ETag | 1 x 200, 49 x 412, 0 x 5xx | IT-20-2 |
| cross-account | account B calls all four with A's vault id | 404 on every call | ST-20-1, AC-59 |
| device-token | device token on POST `/vaults` and PUT header | 403 | ST-20-1 |
| report-ids-only | unlock-failure POST with a passphrase-shaped body | success status; audit row and logs hold ids and time only | ST-20-2 |

## Open points (closed by E-30/c06 with the E-21 and E-02 owners)

1. Wire encoding of the header bytes in request and response bodies (raw body with metadata in headers, or JSON with an encoded field) and its body class (`Small` is 64 KiB); not specified by 20 §4.
2. String form of the `ETag` value (quoted, hex or base64 of the SHA-256); not specified.
3. Status and problem code of `HeaderTooLarge`, of a file given as root, of a second vault on one root and of `VaultExists` (if one vault per account is decided by E-30/c01); IT-20-1 and IT-20-7 say only "4xx".
4. Success status of the unlock-failure report.
5. The problem codes for 412 and 428 are not yet rows of the `sc_error` table (`sc_error.md` open point 1); E-30/c06 adds them.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial draft from 20 §3-4 and tests | decompose Step 2 (B11) |
