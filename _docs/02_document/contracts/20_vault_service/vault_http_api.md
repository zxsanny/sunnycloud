# Contract: vault_http_api — `/api/v1/vaults` routes [v1.2]

**Component**: 20_vault_service (routes registered in 11's registry, served by 11, wired by 12)
**Producer task**: E-30/c06 — `_docs/02_tasks/todo/B11-21_vault_routes_registration.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-30/c08 (integration and security suite), E-30/p01 (`AZ-3675_pipeline_vault`), the client vault pages of 16 (E-26) and 17/18 (E-27, E-28) that call these routes with header bytes produced by `vault-core` (19) — those client tasks are not planned yet (flagged on AZ-3675)
**Version**: 1.1.0
**Status**: frozen (open points 1-5 closed by E-30/c06, AZ-3926, owner decisions 2026-11-18); after v1.2 ships the E-21 additive-only diff gate applies
**Last Updated**: 2026-11-18

## Purpose

The four HTTP operations through which a client creates a vault, reads and replaces its opaque header with optimistic concurrency, and reports an unlock failure. Clients (16, 17, 18) and the vault core's callers depend on the exact status and header rules, so they read this file, not the producer task. Sources: `components/20_vault_service/description.md` §3, §4, §7; `epics/E-30_vault_service.md`; `components/20_vault_service/tests.md`; `contracts/11_http_api/api_v1_surface.md`.

## Shape

### HTTP endpoints

| Method | Path | Auth class | Mut | Port method | Request | Success response | Error statuses |
|--------|------|-----------|-----|-------------|---------|------------------|----------------|
| `POST` | `/api/v1/vaults` | `SessionOnly` | yes | `VaultService::create_vault` | `{"root_node_id": uuid, "header": base64}`, body class `Small` | 201; `ETag`; `{"id": uuid}` | 400 `invalid_request` (body shape; `header` not base64 or empty), 401, 403 (device token; CSRF), 404 `not_found` (root unknown, foreign, tombstoned, a file or the account's own root), 409 `vault_exists` (the account already has a vault, or the root already holds one), 413 `vault_header_too_large` |
| `GET` | `/api/v1/vaults/{id}` | `Session` | no | `VaultService::get_header` | none | 200; `ETag`; `{"id": uuid, "root_node_id": uuid, "header": base64}` | 400 (`id` not a UUID), 401, 403, 404 |
| `PUT` | `/api/v1/vaults/{id}/header` | `SessionOnly` | yes | `VaultService::replace_header` | `{"header": base64}`, body class `Small`; `If-Match` request header required | 200; new `ETag`; empty body | 400, 401, 403, 404, 412 `precondition_failed` (`If-Match` differs from the stored ETag; stored header unchanged), 428 `precondition_required` (no `If-Match`; no port call, stored header unchanged), 413 `vault_header_too_large` |
| `POST` | `/api/v1/vaults/{id}/unlock-failures` | `Session` | yes | `VaultService::report_unlock_failure` | no fields, body class `Small`; the body is never read, stored or logged | 204 No Content | 400 (`id` not a UUID), 401, 403, 404 (unknown or foreign id; nothing written) |

`x-id-params`: `{id}` (path) on the last three operations; `root_node_id` (body) on create.

### Headers

| Header | Direction | Rule |
|--------|-----------|------|
| `ETag` | response of create, GET and PUT | strong tag: the lowercase hex SHA-256 of the stored header bytes in double quotes, `"<64 hex>"`; equal on every read of unchanged bytes. 20 returns the bare hex; 11 adds the quotes |
| `If-Match` | request of PUT | required. Absent: 428, no port call. One line holding exactly one quoted tag: the text between the quotes is compared with the stored ETag at write time. Any other present value (empty, unquoted, weak `W/"…"`, `*`, a list, several lines) is compared as an empty tag, which matches no stored header, so it answers 412 and writes nothing |

### Field table (request and response values)

| Field | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| header | bytes, standard base64 with padding on the wire (JSON string, OpenAPI `format: byte`) | yes (create, PUT; GET response) | opaque authenticated vault header from `vault-core` | 1 to `max_vault_header_bytes` bytes (default 1,024, so the JSON body stays far below the 64 KiB `Small` class); never parsed or altered by the server; text that is not base64 is 400 `invalid_request` (`header`/`not_base64`), an empty header 400 (`header`/`empty`) |
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
| report-ids-only | unlock-failure POST with a passphrase-shaped body | 204; audit row and logs hold ids and time only | ST-20-2 |

## Open points (closed by E-30/c06, AZ-3926, owner decisions 2026-11-18)

1. Closed: JSON bodies with the header in standard base64 (`header`), body class `Small` (64 KiB). Create `{"root_node_id", "header"}`, replace `{"header"}`, GET answers `{"id", "root_node_id", "header"}`.
2. Closed: strong `ETag`, the quoted lowercase hex SHA-256, `"<64 hex>"`; the `If-Match` reading is the row in "Headers".
3. Closed: `HeaderTooLarge` is 413 `vault_header_too_large`; `VaultExists` (the caller already has its one vault, whatever root it names) is 409 `vault_exists`; another account's vault root, a file, foreign, tombstoned or unknown root, and the account's own root, are the port's `NotFound`, 404 `not_found`.
4. Closed: the unlock-failure report answers 204 No Content; its body is ignored.
5. Closed: `precondition_failed` (412) and `precondition_required` (428), with `vault_exists` (409) and `vault_header_too_large` (413), are rows of the `sc_error` table (`sc_error.md` 1.1.0).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial draft from 20 §3-4 and tests | decompose Step 2 (B11) |
| 1.1.0 | 2026-11-18 | Open points 1-5 closed (base64 JSON wire, quoted SHA-256 `ETag`, the `If-Match` reading, 409/413/404 rows, 204 report, the four `sc_error` rows); status frozen | AZ-3926 |
