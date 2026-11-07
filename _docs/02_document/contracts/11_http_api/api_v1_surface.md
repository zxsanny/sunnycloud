# Contract: api_v1_surface — `/api/v1` operations, OpenAPI document and cross-route rules

**Component**: 11_http_api
**Producer task**: E-21/c11 — `_docs/02_tasks/todo/B07-11_route_registry_openapi.md` (contributors: E-21/c04 security headers, E-21/c05 body classes and timeouts, E-21/c07 auth classes and credential transport, E-21/c08 CSRF rule, E-21/c12, c13, c14, c15 route rows); keys assigned at Step 4
**Consumer tasks**: E-21/c18 (CSRF and IDOR generators), E-15/c13 (scope declaration check), E-30/c06 (vault routes, v1.2), E-23 (`HttpSyncClient`, all operations it calls), E-26/c06 (generated `ApiClient`), E-24 and E-27/E-28 (through `sync-core` and `mobile_backup`), E-20/c03 (worker routes), E-31 (FT-P-70, NFT-SEC-09, NFT-SEC-13, NFT-SEC-50); resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The one list of HTTP operations the server offers, with the class rules every client must follow. The route registry is the single source: the router, the OpenAPI document and the CSRF and two-account test matrices are all generated from it, so a route cannot exist without being documented and tested (AC-58, AC-59, AC-64). Clients are generated from, or checked against, the OpenAPI document; they import no server crate.

Sources: `components/11_http_api/description.md` §5, §8, §9; component specs 01 §3, 02 §3, 03 §5, 05 §3, 06 §3, 07 §4, 08 §5 (each owner's route table is authoritative for its paths, 11 §8); `deployment/observability.md` § Endpoints.

## Shape

### OpenAPI document

- Served read-only at `GET /api/v1/openapi.json` (Public) and printed by `sunnycloud openapi` through `HttpServer::openapi()`; both are the same bytes (answers G-15 for the document URL).
- OpenAPI 3.x, validator-clean. It describes the public router and the internal (worker) router; ops (`:9090`) and test-clock (`:9091`) routes are outside it.
- Security schemes: cookie session, bearer session, bearer device, bearer worker.
- Every operation carries: its auth class, its body class, the owner port method, the state-changing flag, and `x-id-params` (every ID parameter in path, query, body or header). The extension names other than `x-id-params` are fixed by E-21/c11 and recorded here when it is accepted (Open point 1).
- Additive-only between releases: CI diffs the document against the last release tag and fails on a removed operation, a removed response field, a removed enum value or a newly required request field; new optional fields and new operations pass.

### Credential transport and auth classes

| Credential | Sent as | Auth classes it can satisfy |
|------------|---------|-----------------------------|
| Browser session | `__Host-` session cookie (`Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain`), attributes taken from 01's DTO | `Session`, `SessionOrDevice`, `SessionOnly`, `Admin` (role admin) |
| Native session (mobile) | `Authorization: Bearer` session access token | same as browser session |
| Device token (desktop) | `Authorization: Bearer` device token | `SessionOrDevice`, `DeviceOnly` |
| Share grant | path-scoped HttpOnly cookie set after password verification (06) | `ShareGrant` |
| Worker | `Authorization: Bearer <worker_api_token>` plus `X-Worker-Id`, internal listener only | `Worker` |

| Auth class | Rule |
|------------|------|
| `Public` | no credential needed |
| `Session` | session (cookie or bearer) |
| `SessionOrDevice` | session or device token |
| `SessionOnly` | session; a device token gets 403 (AC-57) |
| `DeviceOnly` | device token; a session gets 403 (Open point 2) |
| `Admin` | session with role admin; non-admin 403 |
| `ShareGrant` | 06 resolves token or grant |
| `Worker` | 08 `WorkerAuthenticator`; any other credential or none gets one uniform 403 |

No credential on a protected route gives 401; a revoked, expired or unknown credential gives 401 with the same body; a wrong kind gives 403. Auth is checked before the body is read.

### Cross-route rules

| Rule | Behaviour |
|------|-----------|
| Version header | `X-Sunnycloud-Client` and the gate (`server_info.md`) |
| CSRF (AC-58) | cookie-authenticated POST, PUT, PATCH, DELETE pass only with `Sec-Fetch-Site: same-origin` or `Origin` equal to `public_url`; anything else, or neither header, is 403 `csrf_rejected`. Bearer requests are exempt. GET and HEAD never change state (Open point 3 for the provider callback) |
| CORS | no `Access-Control-*` header on `/api`, preflight included; only public share content may answer `Access-Control-Allow-Origin: *` without credentials |
| Security headers | every response: `Strict-Transport-Security: max-age=31536000`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` (content routes: `private`), `X-Request-Id` (fresh UUIDv7) |
| Errors | `problem_details.md` |
| Content | `content_serving_headers.md` |
| Naming | plural lowercase kebab nouns, no trailing slash, filters in the query; only `commits:batch` and `revisions:lookup` use the colon form; no route outside `/api` except ops, test-clock and the SPA fallback |

### Body classes and time limits

| Class | Limit | Used by |
|-------|-------|---------|
| `Small` | 64 KiB | JSON default |
| `Commit` | 4 MiB | `POST /commits` |
| `Batch` | 4 MiB (500 entries) | `POST /commits:batch` |
| `Block` | exactly the block length for the index, at most 2 MiB; `Content-Length` required (411) | block PUT |
| `Staged` | at most 2 MiB (0 allowed) | staged PUT |
| `Query` | 128 KiB | `thumbnail-states` |
| `Auth` | 16 KiB | sign-in, code exchange, refresh, device-grant and device-token, password verification |
| `WorkerOutput` | per-kind cap from 08 (`derived_max_*`) | worker outputs |
| `None` | no body | GET, HEAD, body-less DELETE |

A declared length over the class limit is 413 before any byte is read (for `Batch`, 4 MiB; more than `batch_max_entries` entries inside a body that fits is 422 `batch_too_large` from the commit service). Body idle timeout 30 s (`Block`, `Staged`); long-poll allows `wait` + 5 s; downloads have a 60 s write-idle timeout and no total cap; every other request 60 s.

### Operations

Abbreviations: auth class `Pub`, `S`, `SoD` (SessionOrDevice), `SO` (SessionOnly), `DO` (DeviceOnly), `Adm`, `SG` (ShareGrant), `W`; body class as above; "Mut" = state-changing.

| Group (owner task) | Method | Path | Auth | Body | Mut | Owner port method |
|--------------------|--------|------|------|------|-----|-------------------|
| System (c06, c11) | GET | `/api/v1/server-info` | Pub | None | no | `ServerInfo` value |
| System (c11) | GET | `/api/v1/openapi.json` | Pub | None | no | `HttpServer::openapi` |
| Auth (c12) | GET | `/api/v1/auth/providers` | Pub | None | no | `SignInService::list_providers` |
| Auth (c12) | POST | `/api/v1/auth/login-attempts` | Pub | Auth | yes | `SignInService::begin` |
| Auth (c12) | GET | `/api/v1/auth/providers/{provider}/callback` | Pub | None | see Open point 3 | `SignInService::complete` |
| Auth (c12) | POST | `/api/v1/auth/mobile-code-exchanges` | Pub | Auth | yes | `SignInService::exchange_mobile_code` |
| Auth (c12) | POST | `/api/v1/auth/refresh-tokens` | Pub | Auth | yes | `SessionService::refresh` |
| Sessions (c12) | GET | `/api/v1/sessions` | SO | None | no | `SessionService::list` |
| Sessions (c12) | DELETE | `/api/v1/sessions/{id}` | SO | None | yes | `SessionService::revoke` |
| Sessions (c12) | DELETE | `/api/v1/sessions/current` | SO | None | yes | `SessionService::revoke` (own session, logout) |
| Identity (c12) | POST | `/api/v1/identity-link-attempts` | SO | Small | yes | `SignInService::begin_link` |
| Devices (c12) | POST | `/api/v1/device-grants` | Pub | Auth | yes | `DeviceGrantService::request` |
| Devices (c12) | POST | `/api/v1/device-tokens` | Pub | Auth | yes | `DeviceGrantService::poll` |
| Devices (c12) | GET | `/api/v1/device-grants?user_code=` | SO | None | no | `DeviceGrantService::lookup_for_approval` |
| Devices (c12) | PATCH | `/api/v1/device-grants/{id}` | SO | Small | yes | `DeviceGrantService::decide` |
| Devices (c12) | GET | `/api/v1/devices` | SO | None | no | `DeviceGrantService::list_devices` |
| Devices (c12) | DELETE | `/api/v1/devices/{id}` | SO | None | yes | `DeviceGrantService::revoke_device` |
| Devices (c12) | DELETE | `/api/v1/devices/me` | DO | None | yes | `DeviceGrantService::revoke_self` |
| Account (c12) | GET | `/api/v1/accounts/me` | SoD | None | no | `AccountService::view` |
| Account (c12) | DELETE | `/api/v1/accounts/me` | SO | Small | yes | `AccountDeletionService::request` (202) |
| Account (c12) | GET | `/api/v1/accounts/me/quota` | SoD | None | no | `AccountService::quota_view` |
| Admin (c12) | GET | `/api/v1/admin/users` | Adm | None | no | `AdminService::list_users` |
| Admin (c12) | PATCH | `/api/v1/admin/users/{id}` | Adm | Small | yes | `AdminService::update_user` |
| Admin (c12) | PUT | `/api/v1/admin/users/{id}/quota` | Adm | Small | yes | `AdminService::set_quota` |
| Admin (c12) | GET | `/api/v1/admin/health` | Adm | None | no | `AdminService::health` |
| Admin (c12) | POST, GET | `/api/v1/admin/invites` | Adm | Small / None | yes / no | `InviteService::create_invite` / `list` |
| Admin (c12) | DELETE | `/api/v1/admin/invites/{id}` | Adm | None | yes | `InviteService::revoke` |
| Tree (c13) | GET | `/api/v1/nodes/root` | SoD | None | no | `TreeReader::get_node(Root)` |
| Tree (c13) | POST | `/api/v1/nodes` | SoD | Small | yes | `TreeService::create_folder` |
| Tree (c13) | GET | `/api/v1/nodes/{id}` | SoD | None | no | `TreeReader::get_node` |
| Tree (c13) | PATCH | `/api/v1/nodes/{id}` | SoD | Small | yes | `TreeService::update_node` |
| Tree (c13) | DELETE | `/api/v1/nodes/{id}?base_revision=` | SoD | None | yes | `TreeService::delete_node` |
| Tree (c13) | GET | `/api/v1/nodes/{id}/children?after=&limit=` | SoD | None | no | `TreeReader::list_children` |
| Tree (c13) | GET | `/api/v1/revisions/{id}` | SoD | None | no | `TreeReader::get_revision` |
| Tree (c13) | GET | `/api/v1/revisions?client_device=` | SoD | None | no | `TreeService::find_revisions` |
| Tree (c13) | POST | `/api/v1/revisions:lookup` | SoD | open (Open point 4) | no | `TreeReader::lookup_revisions` |
| Transfer (c13) | POST | `/api/v1/uploads` | SoD | Small | yes | `UploadService::start_upload` |
| Transfer (c13) | GET, DELETE | `/api/v1/uploads/{upload_id}` | SoD | None | no / yes | `UploadService::get_upload` / `abort_upload` |
| Transfer (c13) | PUT | `/api/v1/uploads/{upload_id}/blocks/{index}` | SoD | Block | yes | `UploadService::put_block` (`X-Blake3`, 204) |
| Transfer (c13) | PUT | `/api/v1/staged-uploads/{staged_key}` | SoD | Staged | yes | `UploadService::stage_small` |
| Transfer (c13) | POST | `/api/v1/commits` | SoD | Commit | yes | `CommitService::commit_upload` |
| Transfer (c13) | POST | `/api/v1/commits:batch` | SoD | Batch | yes | `CommitService::commit_batch` |
| Transfer (c13) | GET, HEAD | `/api/v1/nodes/{node_id}/content` | SoD | None | no | `DownloadService::open_revision_content(Current)` |
| Transfer (c13) | GET, HEAD | `/api/v1/revisions/{revision_id}/content` | SoD | None | no | `DownloadService::open_revision_content(Revision)`, `retained_ok` only for admin |
| Sync (c13) | GET | `/api/v1/changes?cursor=&wait=&limit=` | SoD | None | no | `ChangeFeed::read` |
| Sync (c13) | GET | `/api/v1/changes/head` | SoD | None | no | `ChangeFeed::head` |
| Sharing (c14) | POST, GET | `/api/v1/shares` | SO | Small / None | yes / no | `ShareService::create` (201) / `list` |
| Sharing (c14) | DELETE | `/api/v1/shares/{id}` | SO | None | yes | `ShareService::revoke` |
| Sharing (c14) | GET | `/api/v1/public-shares/{token}` | Pub | None | no | `PublicShareService::open` |
| Sharing (c14) | POST | `/api/v1/public-shares/{token}/password-verifications` | Pub | Auth | yes | `PublicShareService::verify_password` |
| Sharing (c14) | GET, HEAD | `/api/v1/public-shares/{token}/content` | SG | None | no | `PublicShareService::authorize_content`, then `DownloadService` |
| Sharing (c14) | GET, HEAD | `/api/v1/public-shares/{token}/thumbnail` | SG | None | no | `PublicShareService::authorize_derived`, `DerivedOpener::open_derived_for`, `DownloadService::open_blob_content` (Open point 5) |
| Media (c14) | GET, HEAD | `/api/v1/nodes/{node_id}/thumbnail?size=small\|preview` | SoD | None | no | `DerivedOpener::open_derived`, then `DownloadService::open_blob_content` |
| Media (c14) | GET, HEAD | `/api/v1/nodes/{node_id}/transcode` | SoD | None | no | same with `Transcode` |
| Media (c14) | POST | `/api/v1/thumbnail-states` | SoD | Query | no | `ThumbnailStateSource::states` (Open point 6) |
| Worker (c15, internal) | POST | `/api/v1/worker-jobs/claims` | W | Small | yes | `WorkerJobs::claim` (200 or 204) |
| Worker (c15, internal) | PUT, DELETE | `/api/v1/worker-jobs/{job_id}/lease` | W | Small / None | yes | `WorkerJobs::extend_lease` / `release` |
| Worker (c15, internal) | GET | `/api/v1/worker-jobs/{job_id}/input` | W | None | no | `WorkerJobs::open_input` |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/{job_id}/outputs/{kind}` | W | WorkerOutput | yes | `WorkerJobs::accept_output` |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/{job_id}/result` | W | Small | yes | `WorkerJobs::complete_skipped` |
| Worker (c15, internal) | POST | `/api/v1/worker-jobs/{job_id}/failure` | W | Small | yes | `WorkerJobs::fail` |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/status` | W | Small | yes | `WorkerJobs::record_status` |
| Test clock (c15, feature `test-clock`) | GET | `/internal/v1/test/clock-offset` (internal listener) | W | None | no | `OffsetClock::offset` |
| Test clock (c15, feature `test-clock`) | POST | `/test/clock` (`:9091`) | Open point 7 | Small | yes | `OffsetClock::advance` |
| Vault (E-30/c06, v1.2) | - | `/api/v1/vaults/**` | S | - | - | `VaultService` (reserved) |
| SPA (c11) | GET | every non-`/api` path | Pub | None | no | bundled web index (fallback) |

## Invariants

- The router and the OpenAPI document contain the same set of (method, path template): 0 undocumented routes, 0 phantom operations.
- Every operation declares exactly one auth class, one body class, its state-changing flag, its owner port method and every ID parameter.
- The public router contains no worker or test-clock route; the internal router contains only the worker routes and, with feature `test-clock`, the clock-offset read; every other path there is 404.
- Handlers take the account from the authenticated principal, never from a path, query or body field; a foreign id is a 404 equal to a random id.
- Paths are stable inside `/api/v1`; the document only grows between releases.

## Non-Goals

- Not covered: request and response bodies of each operation (owners' specs and the generated document), business outcomes, status codes per business error (`problem_details.md`).
- Not covered: ops endpoints (`sc_observability`), the vault handlers (E-30).

## Versioning Rules

- **Breaking**: an operation removed or its path, method, auth class or body class tightened; a field removed; a request field made required. Needs `/api/v2` or a deprecation path; the CI diff gate blocks it.
- **Non-breaking**: a new operation, a new optional field, a looser limit.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| parity | live router table vs `openapi()` | equal sets; served document byte-equal | IT-11-11 |
| declarations | every operation | auth class, body class, flag, owner, `x-id-params` present | IT-11-11, E-15/c13 |
| naming | every path | rules above hold; only the two colon forms | IT-11-11, FT-P-70 |
| diff-gate | removed operation, new required field, removed enum value, removed response field, new optional field, new operation | first four fail, last two pass | IT-11-12 |
| csrf-matrix | every state-changing operation with a cookie and foreign or no origin | 403 `csrf_rejected` on N of N; bearer passes | IT-11-6, NFT-SEC-09 |
| idor-matrix | every (operation, ID parameter) with the other account's id | 404, uniform with a random id | ST-11-3, NFT-SEC-13 |
| listener-split | worker routes on `:8080` and through Caddy; user credentials on `:8081` | 404; uniform 403 | IT-11-9, NFT-SEC-50 |

## Open points

1. Extension names for auth class, body class, state-changing flag and owner port method are not given by 11 §9 (only `x-id-params` is); E-21/c11 fixes them. E-15/c13 reads the owner port method to match operations.
2. `DeviceOnly`: 01's `Scope` has four values (`Public`, `SessionOnly`, `SessionOrDevice`, `Admin`) and lists `DELETE /devices/me` as `SessionOrDevice` with the session refusal inside `DeviceGrantService::revoke_self`; 11 declares `DeviceOnly`. Owners of E-15 and E-21 to decide where the refusal lives; the HTTP answer (403 for a session) is the same either way.
3. The provider callback is a GET by protocol that consumes the state row and creates a session; the "GET never mutates" check needs an explicit, named exemption for it (and for the link completion, whose route 01 does not list).
4. `revisions:lookup` takes up to 1,000 content hashes (about 70 KB as hex JSON), above `Small` (64 KiB); its body class is not assigned (`Query` 128 KiB fits).
5. The public share thumbnail route is named by 11 §8 and 08 §5 but missing from 06's route table; its exact path and the `size` parameter are taken from 11.
6. `thumbnail-states`: 08 describes a body of node ids, while `ThumbnailStateSource::states` takes content hashes; One handler must stay one port call. Decided 2026-11-07 (G-08-3): ids of another account and unknown ids are omitted from the answer; a body over 1,000 ids is 422 `batch_too_large`. The node-id versus content-hash mapping step is still open.
7. Auth on `POST /test/clock` (`:9091`): `observability.md` says "bearer via `e2e-ctl`", but no such credential is defined in 11 or helper 01.
8. `GET /api/v1/setup` (11 §8, owner 01) is not in 01's route table; `list_providers` already returns `setup_required`.
9. 11 §8's indicative paths differ from the owners' (`.../login`, `code-exchanges`, `GET/POST device-grants/{user_code}`, `public-shares/{token}/access`); this contract follows the owners, as 11 §8 says.
10. Request id: 11 (§5 layer 3, IT-11-4) issues a fresh UUIDv7 and does not echo a client `X-Request-Id`; `sc_error::CorrelationId::from_request` keeps a valid incoming value, `sc_observability` AC-3 assumes a caller-set id, and 16's `ApiClient` sends one. Owners of E-02, E-09 and E-21 to decide.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
