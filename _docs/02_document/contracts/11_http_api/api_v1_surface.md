# Contract: api_v1_surface — `/api/v1` operations, OpenAPI document and cross-route rules

**Component**: 11_http_api
**Producer task**: E-21/c11 — `_docs/02_tasks/todo/B07-11_route_registry_openapi.md` (contributors: E-21/c04 security headers, E-21/c05 body classes and timeouts, E-21/c07 auth classes and credential transport, E-21/c08 CSRF rule, E-21/c12, c13, c14 route rows; E-21/c15 worker and test-clock rows, `_docs/02_tasks/todo/AZ-3831_internal_router_test_clock.md`); keys assigned at Step 4
**Consumer tasks**: E-21/c18 (CSRF and IDOR generators), E-15/c13 (scope declaration check), E-30/c06 (vault routes, v1.2), E-23 (`HttpSyncClient`, all operations it calls), E-26/c06 (generated `ApiClient`), E-24 and E-27/E-28 (through `sync-core` and `mobile_backup`), E-20/c03 (worker routes), E-31 (FT-P-70, NFT-SEC-09, NFT-SEC-13, NFT-SEC-50); resolved at Step 4
**Version**: 1.8.0
**Status**: draft
**Last Updated**: 2026-11-11

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
| CSRF (AC-58) | cookie-authenticated POST, PUT, PATCH, DELETE pass only with `Sec-Fetch-Site: same-origin` or `Origin` equal to `public_url`; anything else, or neither header, is 403 `csrf_rejected`. Bearer requests are exempt. GET and HEAD never change state (Open point 3 for the provider callback). Enforcement (layer 9, after auth, before the body limits): the method decides, not the state-changing flag; "cookie-authenticated" means the credential auth accepted came from the session cookie (a request with a bearer and a cookie is a bearer request); a route with no authenticated caller (`Public`, `ShareGrant`, `Worker`) is not checked; an invalid or revoked cookie is the auth answer (401), not 403. `Origin` equals `public_url` when scheme, host and port are equal (host case-insensitive, default port elided); a path, query, user info, `null` or a repeated `Origin` or `Sec-Fetch-Site` header never matches. `public_url` must be `http(s)://host[:port]` with at most one trailing slash, else the server does not start. The `Host` header is never read |
| CORS | no `Access-Control-*` header on `/api`, preflight included; only the bytes of public share content (`GET` and `HEAD` `/api/v1/public-shares/{token}/content`, status 200, 206 and 304) answer `Access-Control-Allow-Origin: *`, never `Access-Control-Allow-Credentials`; its errors, the share thumbnail and every other route carry none |
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

Enforcement (E-21/c05), per route, innermost layer in front of the handler and behind the version gate and auth, so a refused caller never has a body byte read:

- A declared `Content-Length` over the class limit is 413 `size_too_large` with no body byte read, counted once in `body_limit_exceeded_total`. A body with no declared length (chunked) is streamed to the handler and cut at the limit: the request ends 413 `size_too_large` (also counted) and the handler's own answer is discarded. `None` has limit 0, so a GET or HEAD with a body is refused when it declares one and otherwise never read.
- `WorkerOutput` is capped by the `{kind}` of the route (`thumb`, `preview`, `transcode`) from `HttpConfig.worker_output_limits` (config keys `derived_max_thumb`, `derived_max_preview`, `derived_max_transcode`); any other kind gets the largest of the three, and 08 answers its own kind rejection.
- Clocks, by the `Timing` an operation declares (default `Request`):

| Timing / class | Body idle | Total handler time |
|----------------|-----------|--------------------|
| `Request`, class `Block` or `Staged` | `body_idle_timeout` | none |
| `Request`, any other class | none | `request_timeout` |
| `LongPoll` (a GET with a `wait` query parameter) | none | `min(wait, longpoll_wait_max)` + 5 s; `request_timeout` when `wait` is absent or not a number. `wait` is read in whole seconds, the unit `longpoll_max_wait` is served in |
| `Download` (GET, HEAD) | none | none; a response write that makes no progress for `download_write_idle` closes the connection (below) |

- The idle timer runs only while the handler waits for body data and the peer sends nothing; it restarts at every frame. A body that goes idle, or a handler that passes its total time, ends the request with a reset (RST_STREAM on HTTP/2, connection close on HTTP/1.1; the carrier response head is 500 and its body fails at once), never a clean answer. No status is fixed for either case (Open point 12).
- A block PUT without `Content-Length` (including a chunked one) is answered 411 `length_required` before any body byte is read.
- The write-idle timer is on the connection's socket, not the route: any response write or flush that makes no progress for `download_write_idle` fails and closes the connection (TCP; HTTP/2 shares it for all streams of the connection). It is the only place that notices a reader that stopped reading. Only a download can be large enough to stall, so no other route is affected in practice.

### Worker routes and the test clock (E-21/c15)

Worker routes are registered with the internal listener only; the registry validates that the internal listener holds only `Worker` routes under `/api/v1/worker-jobs/` and `/internal/v1/test/`, and that the public router holds none. Every other path on the internal listener is the same 404 problem as an unknown path on the public one.

- Auth: `Worker` class, 08 `WorkerAuthenticator` only; no credential, a cookie, a session or device token and a wrong secret all get the one 403 `forbidden`, and no `WorkerJobs` method runs. A call that passes auth must carry `X-Worker-Id` exactly once and non-empty, else 400 `invalid_request` (field `x-worker-id`); it is passed to the port unchanged. A `job_id` that is not a UUID is 400 `invalid_request` (field `job_id`).
- Bodies are JSON with exactly the fields of 08 section 5 (`{lane}`, `{outcome}`, `{code, disposition}`, `{decoder_ok, image_age_days}`); an unknown field, a missing field or another value is 400 `invalid_request`. `claims` answers 200 `ClaimedJob` (`job_id`, `kind`, `content_hash` as 64 lowercase hex digits, `params_version`, `attempt`, `lease_ms`) or an empty 204; the lease PUT answers 200 `{lease_ms}`; `failure` answers 200 `{result}`.
- Port errors: `LeaseLost` 409 `lease_lost`, `RevisionObsolete` 404 `revision_obsolete`, `OutputTooLarge` 413 `output_too_large`, `OutputTypeRejected` 422 `output_type_rejected`, `OutputKindRejected` 422 `output_kind_rejected`, `Storage` and `Unexpected` an opaque 500 (`problem_details.md`).
- `input` is `Timing::Download`: always 200 with the whole decrypted body and the exact `Content-Length` the port reports; `Range` is never read (G-08-4).
- `outputs/{kind}`: `kind` is `thumb`, `preview` or `transcode`; any other value is 422 `output_kind_rejected` without a port call. `Content-Length` is required (a chunked body is 411 `length_required`, no byte read); `Content-Type` is required and is handed to the port as the media type; `X-Width`, `X-Height` and `X-Duration-Ms` are optional numbers (not a number: 400). The body reaches the port as a stream, cut at the `WorkerOutput` cap of its `{kind}`.
- Every answer carries `Cache-Control: no-store` (layer 5 default).
- Test clock, feature `test-clock` only: the clock-offset read is an ordinary `Worker` registry operation (so it is in the document of a test build and absent from a release one); `POST /test/clock` is one router on the `:9091` listener (`HttpConfig.test_clock_addr`), outside the registry and the document, with body `{"advance_seconds": N}` (a whole non-negative number; negative or out of range is 400 `invalid_request`, the offset unchanged) and answer 200 `{"offset_seconds": <new offset>}`; the read answers the same shape. A release build compiles neither route nor the listener: `:9091` is not bound, and both paths are 404 on the listeners that exist.

### Sharing and media routes (E-21/c14)

- **Owner routes** (`SessionOnly`; a device token is 403 `forbidden`, AC-57; a cookie session passes CSRF like any unsafe method): `POST /shares` takes `{node_id, expires_in?, password?}` (`expires_in` in whole seconds, 06 applies its default and maximum; an unknown field, a wrong type or a non-UUID node is 400 `invalid_request`) and answers 201 `{id, token, url_path, expires_at, has_password}`; this answer is the only one that ever carries the token. `GET /shares?node_id=` answers 200 `{items: [{id, node_id, expires_at, has_password, created_at}]}`, never a token. `DELETE /shares/{id}` answers 204 (idempotent for the owner's own link; a foreign or unknown id is the uniform 404). `TargetNotFound` and `NotFound` are 404 `not_found`, `TargetNotShareable` 422 `target_not_shareable`, `ExpiryOutOfRange` 400 `invalid_request` (field `expires_in`). Timestamps are RFC 3339 UTC with microseconds and `Z` (Open point 22).
- **Visitor routes** (no account; 11 passes 06 the token of the path, the client address of the `ClientIp` layer and the grant): `GET /public-shares/{token}` answers 200 `{name, size, media_type, expires_at}` for an available link, 401 `password_required` for a live password link (no file data in the body), and the uniform 404 for `NotFound`. `POST .../password-verifications` takes `{password}` (class `Auth`, 16 KiB) and answers 204 with the grant cookie, 401 `password_required` for a wrong password, 404 for an unknown link, 429 from 06's counters. `GET` and `HEAD .../content` call `authorize_content` and then `DownloadService::open_revision_content(Revision(revision_id))` for the account 06 returns, with `retained_ok` false.
- **One 404** (AC-2, AC-12): an unknown, expired, revoked or deleted link, and a path that is not text, are `ApiProblem::not_found`: status, headers and body equal apart from `request_id` on `open`, `verify_password` and `content`. 11 checks no token format, so no input is told apart before 06 decides.
- **Grant cookie** (Open point 23): after a correct password the answer carries `Set-Cookie: __Secure-share-grant=<expires_at in unix microseconds>.<grant value>; HttpOnly; Secure; SameSite=Lax; Path=/api/v1/public-shares/{token}; Max-Age=<seconds until expires_at>`. 06 wants the whole `AccessGrant` back (its value is an HMAC over the link and `expires_at`), so the cookie carries both. A cookie that is missing or not of that form is no grant, and 06 answers `password_required` for a protected link. The cookie reaches only its own link's routes; no other route or link ever receives it. Grants and tokens that cannot be written into a header (a character outside `cookie-octet`, or outside `A-Z a-z 0-9 - _ . ~` in the token) are a broken 06 answer: opaque 500, logged.
- **`starts_download`** is true when the request is not a `HEAD` and has no usable `Range` or a range that starts at byte 0, so one download start writes one `share_accessed` however many ranged requests follow (`content_serving_headers.md`, request side).
- **Share token redaction** (NFT-SEC-16): the metric label and the log field of a request are the route template (`/api/v1/public-shares/{token}`), looked up from the registry, never the request path; no handler logs the token, and a request to a path no template matches is `unmatched`. Every share route is in the registry, so every one is covered. IT: the token appears in no log line and no metric of the visitor routes.
- **Node media routes** (`SessionOrDevice`): `GET` and `HEAD /nodes/{node_id}/thumbnail?size=small|preview` (`size` absent is `small`, any other value 400 `invalid_request`) call `DerivedOpener::open_derived(account, node_id, Thumb | Preview)` and then `DownloadService::open_blob_content(account, blob, media_type, options)`; `/nodes/{node_id}/transcode` does the same with `Transcode`. 08's `NotReady` is 404 `thumbnail_not_ready`, `NotFound` 404 `thumbnail_unavailable`; every other answer is the content mapper's (Range and validators included). The media type handed to 03 is the one 08 produces per kind: `image/webp` for `Thumb` and `Preview`, `video/mp4` for `Transcode` (Open point 26).
- **`POST /thumbnail-states`** (`SessionOrDevice`, class `Query`) takes `{content_hashes: [<64 hex digits>, ...]}` and answers 200 `{states: {<hash>: ready | pending | failed}}` from one `ThumbnailStateSource::states` call for the principal's account. More than 1,000 hashes is 422 `batch_too_large`; a value that is not 64 hex digits is 400 `invalid_request` (field `content_hashes`). A hash whose state is `None` (no job planned, or unknown to the account, which includes a hash of another account) is left out of the answer (Open point 6).

### Tree, transfer and sync routes (E-21/c13)

All 22 operations are `SessionOrDevice`. Each handler binds, makes one port call with the account of the principal (never an id from the path, query or body: a body member that is not in the list below is 400 `invalid_request`) and maps the result; nothing is awaited between a port's return and the response (AC-69). A request that does not bind is 400 with the field, before any port call (`problem_details.md`). Query values are read as written; `after` and `client_device` are percent-decoded (a `+` stays a `+`). Hashes are 64 hex digits, either case in, lower case out; timestamps in answers are RFC 3339 UTC with microseconds, and `client_mtime` in a request is `YYYY-MM-DDTHH:MM:SS[.f]Z` (up to six digits, UTC only). `client_meta` is any JSON value on the wire and its text in the ports.

- **Nodes** (02): `NodeView` is `{node_id, parent_id, kind, name, state, current_revision, size, content_hash, media_type, client_mtime, created_at, updated_at, seq}` (`kind` `file` or `folder`, `state` `live` or `tombstone`, file-only members `null` for a folder). `GET /nodes/root` and `GET /nodes/{id}` answer it; `POST /nodes` takes `{parent_id, name}` and answers 201; `PATCH /nodes/{id}` takes `{name?, parent_id?}` (a `null` is an absent member; what neither means is 02's) and answers 200; `DELETE /nodes/{id}?base_revision=<revision id>` answers 204 (a stale base on a file is 409 `edited_since`). `GET /nodes/{id}/children?after=<key>&limit=<n>` answers `{items: [NodeView...], next}` with `next` the opaque key for `after`, `null` on the last page; `limit` absent is 1000 (02's `listing_default_limit`, which the port takes as a number).
- **Revisions** (02): `GET /revisions/{id}` answers `{revision_id, node_id, size, content_hash, blocklist: [hex], media_type, client_mtime, client_meta, created_at}`. `GET /revisions?client_device=&offset=&limit=` answers `{items: [{revision_id, node_id}]}` (`offset` absent is 0, `limit` absent is 1000). `POST /revisions:lookup` takes `{content_hashes: [hex...]}` in class `Query` (128 KiB; 1,000 hashes are about 70 KB), reads only, and answers an array `[{revision_id, node_id, content_hash, client_meta}]`; more than 1,000 hashes is 02's 422 `batch_too_large`.
- **Uploads** (03): `POST /uploads` takes `{parent_id, name, declared_size, client_key, content_hash?, base_revision?, client_meta?}` and answers 200 `{outcome: "session", upload_id, need_blocks, block_size, lane_limit, expires_at}` or `{outcome: "nothing_to_send", revision_id}`. `GET /uploads/{upload_id}` answers `{upload_id, need_blocks, received_count, expires_at}`; `DELETE` answers 204. `PUT /uploads/{upload_id}/blocks/{index}` needs `X-Blake3` (64 hex digits) and an exact `Content-Length` (411 without it); the body reaches `put_block` as a stream, as it arrives, with the length and hash, and 204 is sent only after the port returns its acknowledgement. `PUT /staged-uploads/{staged_key}` takes the same body without `X-Blake3` for a zero-byte file (03 decides what a missing hash on bytes means), its length is the `Content-Length`, or the exact size of a body that has no such header, and it answers 200 `{staged_key, size}`.
- **Commits** (03): `POST /commits` takes `{upload_id, parent_id, name, blocklist: [hex...], base_revision?, client_mtime?, client_meta?, import_hint?}` and answers 200 `{outcome: "published", node_id, revision_id}` or, when the port reports a conflict copy, `{outcome: "conflict_copy", node_id, revision_id, copy_node_id}`. `POST /commits:batch` takes `{commit_id, entries: [{staged_key, parent_id, name, base_revision?, client_mtime?, client_meta?, import_hint?}]}` (`import_hint` a boolean, absent false) and answers 200 `{entries: [...]}`, one per entry in order: `{status: "committed", node_id, revision_id, conflict_copy}` or `{status: "entry_error", code}` with the wire code of the entry's failure. `CommitActor{account_id, device_label}` is the principal's account and `Principal.label` for both commits, which is the `device` of a conflict-copy name. `need_blocks` is 409 with the indexes; the other refusals are the status table's (`problem_details.md`).
- **Content** (03): `GET` and `HEAD /nodes/{node_id}/content` open `Current(node)`; `/revisions/{revision_id}/content` opens `Revision(id)` with `retained_ok` true for an admin session only (AC-5). Both answer through the content header mapper (`content_serving_headers.md`).
- **Feed** (05): `GET /changes?cursor=E.S&wait=<seconds>&limit=<n>` passes the principal, 01's `PrincipalRevalidator` (the one of `ApiServices`), the cursor, `wait` as whole seconds and `limit` (each `None` when absent) to `ChangeFeed::read` and answers 200 `{items: [{node_id, parent_id, kind, name, state, revision_id, size, content_hash, seq}], next_cursor: "E.S", has_more}`. A missing `cursor` is 400, a cursor that is not `E.S` is 422 `invalid_cursor` with no port call (the port takes a parsed `Cursor`, so its `BadCursor` is for a cursor that parses and is still invalid), `ResyncRequired` is 410 with `reason`, `Unauthenticated` mid-hold is 401, `TooManyHolds` is 429 with `Retry-After`. `GET /changes/head` answers `{cursor: "E.S"}`. Every answer is `no-store`. The clock of `/changes` is `min(wait, longpoll_max_wait)` plus 5 s.

### Identity, account and admin routes (E-21/c12)

Each handler binds, makes one port call and maps; the account is the principal's, except that an admin names the account it acts on in the path (`actor` is always the principal). Bodies are JSON with exactly the members below; an unknown member, a missing one or a wrong type is 400 `invalid_request` before any port call. Timestamps are RFC 3339 UTC with microseconds.

- **Cookies** (Open point 36): the names come from `HttpConfig.credentials` (`session_cookie`, `binding_cookie`, both `__Host-`). Both are `Secure; HttpOnly; SameSite=Lax; Path=/` with no `Domain`; the session cookie holds the access token and adds `Max-Age` = the port's `expires_in`, the binding cookie adds nothing. A value that cannot be written into a header is a broken 01: opaque 500, logged.
- **Sign-in**: `GET /auth/providers` answers `{providers: [{key, display_name}], setup_required}`. `POST /auth/login-attempts` takes `{provider, return_to, client_kind: web|android|ios|desktop, invite_token?, setup_token?, code_challenge?}` with the request's `Host`, answers 201 `{authorization_url}` and sets the binding cookie; `RateLimited` is 429 with `Retry-After`. `GET /auth/providers/{provider}/callback` passes the provider, the query as form-decoded pairs in arrival order, the binding cookie (empty when absent, so 01 finds no state) and the `Host`; a `Session` outcome is 302 to `return_to` with the session cookie, a `MobileCode` outcome 302 to `return_to` with `code=<code>` added to its query, and `UntrustedState`, `TrustedFailure{code}` and `InvitationRequired` are 302 to `/login?error=<reason>` (`invalid_request`, the code, `invitation_required`) with no cookie (Open point 39); every other refusal is the port's mapped answer (429 for `RateLimited`). `POST /identity-link-attempts` (session only) takes `{provider}`, passes the principal and answers 200 `{authorization_url}` with the binding cookie.
- **Tokens** (Open point 38): `POST /auth/mobile-code-exchanges` takes `{code, code_verifier}` and `POST /auth/refresh-tokens` takes `{refresh_token}`; both answer 200 `{access_token, refresh_token, expires_in}` (seconds) in the body and set no cookie. An invalid code is 400 `invalid_request` (field `code`); a reused or unknown refresh token is 401.
- **Sessions** (session only): `GET /sessions` answers `{items: [{id, label, created_at, last_seen_at}]}`; `DELETE /sessions/{id}` and `DELETE /sessions/current` answer 204 and call `SessionService::revoke` with reason `User`, the first with the id of the path and the account of the principal, the second with the principal's own session id.
- **Devices**: `POST /device-grants` takes `{device_name, platform}`, passes the client address of the `ClientIp` layer and answers 200 `{device_code, user_code, verification_uri, interval, expires_in}` (seconds); `verification_uri` is `<public_url>/device`, built from the configured `public_url` and never from `Host` (the web app's device approval page, owner decision 2026-11-12, AZ-4231). `POST /device-tokens` takes `{device_code}` and answers 200 `{access_token, token_type: "Bearer"}`; while the grant cannot yield a token the answer is 400 with `code` `authorization_pending`, `slow_down`, `access_denied` or `expired_token` (`problem_details.md`). `PATCH /device-grants/{id}` (session only) takes `{decision: approve|deny}` and answers 204. `GET /devices` answers `{items: [{id, name, platform, created_at, last_seen_at}]}`; `DELETE /devices/{id}` answers 204; `DELETE /devices/me` is `DeviceOnly`, passes the device principal and answers 204 (a session gets 403 at the door, Open point 2). `GET /device-grants?user_code=` (session only) is the approval lookup: it reads the typed code with `sc_ids::UserCode::from_presented` (case, hyphens and spaces ignored; exactly 8 characters of the base-20 alphabet, else 400 for `user_code`; a missing parameter is 400 too), passes the principal, the code and the `ClientIp`, and answers 200 `{grant_id, expires_at, device, requester_ip, requester_location, approver_ip, approver_location, mismatch}` (locations are `null` when unknown); an unknown, expired or decided code is 404 and `RateLimited` is 429 with `Retry-After`.
- **Account**: `GET /accounts/me` answers `{id, display_name, role, identities: [{provider, email}], quota, root_node_id}` and `GET /accounts/me/quota` answers `{limit_bytes, effective_limit, used, reserved}` (`limit_bytes` is `null` for the installation default), both for a session or a device (Open point 37). `DELETE /accounts/me` (session only) takes `{confirmation}` and answers 202 `{deadline}`; `LastAdmin` is 409, a wrong confirmation 400 (field `confirmation`).
- **Admin** (`Admin`; a member or device gets 403, no credential 401): `GET /admin/users?offset=&limit=` (defaults 0 and 1,000, Open point 40) answers `{items: [{id, role, state: active|disabled|deleting, last_login, used, limit}]}`; `PATCH /admin/users/{id}` takes `{role?: user|admin, state?: active|disabled}` and answers 204 (`LastAdmin` 409, `NotFound` 404); `PUT /admin/users/{id}/quota` takes `{limit: <whole bytes>|null}` and answers 204 (`InvalidLimit` 400, `NotFound` 404); `GET /admin/health` answers `{items: [{name, status: ok|degraded|down, detail}]}`. `POST /admin/invites` takes `{note}` and answers 200 `{token}`, the only answer that ever carries the plaintext; `GET /admin/invites` answers `{items: [{id, note, created_at, expires_at, redeemed_at}]}` and never a token; `DELETE /admin/invites/{id}` answers 204.
- **Logging**: no token, code, `return_to`, callback parameter or email is written to a log line or a metric label; the route template is the label, and the ports' `Debug` redact what they hold.

### Operations

Abbreviations: auth class `Pub`, `S`, `SoD` (SessionOrDevice), `SO` (SessionOnly), `DO` (DeviceOnly), `Adm`, `SG` (ShareGrant), `W`; body class as above; "Mut" = state-changing.

| Group (owner task) | Method | Path | Auth | Body | Mut | Owner port method |
|--------------------|--------|------|------|------|-----|-------------------|
| System (c06, c11) | GET | `/api/v1/server-info` | Pub | None | no | `ServerInfo` value |
| System (c11) | GET | `/api/v1/openapi.json` | Pub | None | no | `HttpServer::openapi` |
| Auth (c12) | GET | `/api/v1/auth/providers` | Pub | None | no | `SignInService::list_providers` |
| Auth (c12) | POST | `/api/v1/auth/login-attempts` | Pub | Auth | yes | `SignInService::begin` |
| Auth (c12) | GET | `/api/v1/auth/providers/{provider}/callback` | Pub | None | yes, the one GET (Open point 3) | `SignInService::complete` (302) |
| Auth (c12) | POST | `/api/v1/auth/mobile-code-exchanges` | Pub | Auth | yes | `SignInService::exchange_mobile_code` |
| Auth (c12) | POST | `/api/v1/auth/refresh-tokens` | Pub | Auth | yes | `SessionService::refresh` |
| Sessions (c12) | GET | `/api/v1/sessions` | SO | None | no | `SessionService::list` |
| Sessions (c12) | DELETE | `/api/v1/sessions/{id}` | SO | None | yes | `SessionService::revoke` |
| Sessions (c12) | DELETE | `/api/v1/sessions/current` | SO | None | yes | `SessionService::revoke` (own session, logout) |
| Identity (c12) | POST | `/api/v1/identity-link-attempts` | SO | Small | yes | `SignInService::begin_link` (`complete_link` has no route, Open point 34) |
| Devices (c12) | POST | `/api/v1/device-grants` | Pub | Auth | yes | `DeviceGrantService::request` |
| Devices (c12) | POST | `/api/v1/device-tokens` | Pub | Auth | yes | `DeviceGrantService::poll` |
| Devices (c12) | GET | `/api/v1/device-grants?user_code=` | SO | None | no | `DeviceGrantService::lookup_for_approval` |
| Devices (c12) | PATCH | `/api/v1/device-grants/{id}` | SO | Small | yes | `DeviceGrantService::decide` |
| Devices (c12) | GET | `/api/v1/devices` | SO | None | no | `DeviceGrantService::list_devices` |
| Devices (c12) | DELETE | `/api/v1/devices/{id}` | SO | None | yes | `DeviceGrantService::revoke_device` |
| Devices (c12) | DELETE | `/api/v1/devices/me` | DO | None | yes | `DeviceGrantService::revoke_self` (DeviceOnly, owner decision 2026-11-12) |
| Account (c12) | GET | `/api/v1/accounts/me` | SoD | None | no | `AccountService::view` |
| Account (c12) | DELETE | `/api/v1/accounts/me` | SO | Small | yes | `AccountDeletionService::request` (202) |
| Account (c12) | GET | `/api/v1/accounts/me/quota` | SoD | None | no | `AccountService::quota_view` |
| Admin (c12) | GET | `/api/v1/admin/users` | Adm | None | no | `AdminService::list_users` |
| Admin (c12) | PATCH | `/api/v1/admin/users/{id}` | Adm | Small | yes | `AdminService::update_user` |
| Admin (c12) | PUT | `/api/v1/admin/users/{id}/quota` | Adm | Small | yes | `AdminService::set_quota` |
| Admin (c12) | GET | `/api/v1/admin/health` | Adm | None | no | `AdminService::health` |
| Admin (c12) | POST, GET | `/api/v1/admin/invites` | Adm | Small / None | yes / no | `InviteService::create_invite` (token once) / `list` |
| Admin (c12) | DELETE | `/api/v1/admin/invites/{id}` | Adm | None | yes | `InviteService::revoke` |
| Tree (c13) | GET | `/api/v1/nodes/root` | SoD | None | no | `TreeReader::get_node(Root)` |
| Tree (c13) | POST | `/api/v1/nodes` | SoD | Small | yes | `TreeService::create_folder` |
| Tree (c13) | GET | `/api/v1/nodes/{id}` | SoD | None | no | `TreeReader::get_node` |
| Tree (c13) | PATCH | `/api/v1/nodes/{id}` | SoD | Small | yes | `TreeService::update_node` |
| Tree (c13) | DELETE | `/api/v1/nodes/{id}?base_revision=` | SoD | None | yes | `TreeService::delete_node` |
| Tree (c13) | GET | `/api/v1/nodes/{id}/children?after=&limit=` | SoD | None | no | `TreeReader::list_children` |
| Tree (c13) | GET | `/api/v1/revisions/{id}` | SoD | None | no | `TreeReader::get_revision` |
| Tree (c13) | GET | `/api/v1/revisions?client_device=` | SoD | None | no | `TreeService::find_revisions` |
| Tree (c13) | POST | `/api/v1/revisions:lookup` | SoD | Query (Open point 4) | no | `TreeReader::lookup_revisions` |
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
| Media (c14) | POST | `/api/v1/thumbnail-states` | SoD | Query | no | `ThumbnailStateSource::states`, body of content hashes (Open point 6) |
| Worker (c15, internal) | POST | `/api/v1/worker-jobs/claims` | W | Small | yes | `WorkerJobs::claim` (200 `ClaimedJob` or 204) |
| Worker (c15, internal) | PUT, DELETE | `/api/v1/worker-jobs/{job_id}/lease` | W | Small / None | yes | `WorkerJobs::extend_lease` / `release` |
| Worker (c15, internal) | GET | `/api/v1/worker-jobs/{job_id}/input` | W | None | no | `WorkerJobs::open_input` |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/{job_id}/outputs/{kind}` | W | WorkerOutput | yes | `WorkerJobs::accept_output` (204 `Accepted`, 200 `AlreadyPresent`) |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/{job_id}/result` | W | Small | yes | `WorkerJobs::complete_skipped` (204) |
| Worker (c15, internal) | POST | `/api/v1/worker-jobs/{job_id}/failure` | W | Small | yes | `WorkerJobs::fail` (200 `{result}`) |
| Worker (c15, internal) | PUT | `/api/v1/worker-jobs/status` | W | Small | yes | `WorkerJobs::record_status` (204) |
| Test clock (c15, feature `test-clock`) | GET | `/internal/v1/test/clock-offset` (internal listener) | W | None | no | `OffsetClock::offset` (200 `{offset_seconds}`) |
| Test clock (c15, feature `test-clock`) | POST | `/test/clock` (`:9091`, outside the document) | none, Open point 7 | 1 KiB | yes | `OffsetClock::advance_seconds` (200 `{offset_seconds}`) |
| Vault (E-30/c06, v1.2) | - | `/api/v1/vaults/**` | S | - | - | `VaultService` (reserved) |
| SPA (c11) | GET | every non-`/api` path | Pub | None | no | bundled web index (fallback) |

## Invariants

- The router and the OpenAPI document contain the same set of (method, path template): 0 undocumented routes, 0 phantom operations.
- Every operation declares exactly one auth class, one body class, its state-changing flag, its owner port method and every ID parameter.
- The public router contains no worker or test-clock route; the internal router contains only the worker routes and, with feature `test-clock`, the clock-offset read; every other path there is 404 (the `:9091` router exists only with feature `test-clock` and is not part of either).
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
| share-uniform-404 | open, content, password verification of an unknown, expired, revoked and deleted link | 404, bodies and headers equal apart from `request_id` | AC-2, NFT-SEC-17 |
| share-grant | password link opened, content without grant, password verified, content with the cookie | 401 `password_required` twice with no file data; 204 and an `HttpOnly`, path-scoped cookie; 06 receives the grant | AC-3, NFT-SEC-51 |
| share-token-redaction | every visitor route, failures included | the token is in no log line or metric; the label is the template | NFT-SEC-16 |
| thumbnail-states | 1,000 and 1,001 hashes | one `states` call and 200; 422 `batch_too_large` and no call | G-08-3 |
| listener-split | worker routes on `:8080` and through Caddy; user credentials on `:8081` | 404; uniform 403 | IT-11-9, NFT-SEC-50 |

## Open points

1. Extension names for auth class, body class, state-changing flag and owner port method are not given by 11 §9 (only `x-id-params` is); E-21/c11 fixes them. E-15/c13 reads the owner port method to match operations.
2. `DeviceOnly`: 01's `Scope` has four values (`Public`, `SessionOnly`, `SessionOrDevice`, `Admin`) and lists `DELETE /devices/me` as `SessionOrDevice` with the session refusal inside `DeviceGrantService::revoke_self`; 11 declares `DeviceOnly` (E-21/c12 keeps the class 11 §8 and this table already declared), so a session is refused at the door with 403 and never reaches `revoke_self`, which still refuses it as well. The HTTP answer is the same either way. Owners of E-15 and E-21 to confirm where the refusal lives.
3. The provider callback is a GET by protocol that consumes the state row and creates a session; the "GET never mutates" check needs an explicit, named exemption for it (and for the link completion, which has no route, Open point 34). Registered by E-21/c12 as the one GET declared state-changing. The registry already names `SignInService::complete` as the one GET allowed to be declared state-changing; the CSRF layer checks no GET or HEAD, so it needs no exemption of its own. The generated matrix of E-21/c18 must exempt the same name.
4. Closed by AZ-3829: `revisions:lookup` takes up to 1,000 content hashes (about 70 KB as hex JSON), above `Small` (64 KiB), and is registered with the existing class `Query` (128 KiB), the smallest class that fits; a class of its own would only repeat that limit.
5. The public share thumbnail route is named by 11 §8 and 08 §5 but missing from 06's route table; 11 registers it at `/api/v1/public-shares/{token}/thumbnail` with `size=small|preview` (default `small`, any other value 400 `invalid_request` before any port call). 06 `authorize_derived` returns `ShareDerived{account_id, file}` (`contracts` part 3, 1.1.0, so the account owning the link's file travels with it), `DerivedOpener::open_derived_for(file, Thumb | Preview)` finds the derived blob, `DownloadService::open_blob_content(account_id, blob, image/webp, options)` serves it through the shared header mapper, with no cross-origin header. The path and the `size` query are 11's choice; 06's route table still lacks the row.
6. `thumbnail-states`: 08 describes a body of node ids, while `ThumbnailStateSource::states` takes content hashes; One handler must stay one port call. Decided 2026-11-07 (G-08-3): ids of another account and unknown ids are omitted from the answer; a body over 1,000 ids is 422 `batch_too_large`. The node-id versus content-hash mapping step is still open: no port maps many node ids to hashes in one call (`TreeReader::get_node` is one node per call), so 11 takes **content hashes** in the body, which a listing already gives per file (`NodeView.content_hash`), and answers per hash. A hash of another account is not told apart from an unknown one (`ThumbState::None`) and, like every `None`, is left out of the answer. If the owners want node ids on the wire, 02 or 08 must add a bulk node-id method to a port (a change in `contracts`); the wire would then change from `content_hashes` to ids, a breaking change of this route before any client ships.
7. Auth on `POST /test/clock` (`:9091`): `observability.md` says "bearer via `e2e-ctl`", but no such credential is defined in 11 or helper 01. E-21/c15 implements it with no credential (the listener exists only in test images and no proxy routes it); anyone who can reach `:9091` can move server time. E-21 and E-31 owners to define the credential, or confirm the network boundary is enough.
8. `GET /api/v1/setup` (11 §8, owner 01) is not in 01's route table; `list_providers` already returns `setup_required`. Not registered by E-21/c12: the one route of the first-run page is `GET /auth/providers`. Owners of E-15 and E-21 to confirm or name a second route.
9. 11 §8's indicative paths differ from the owners' (`.../login`, `code-exchanges`, `GET/POST device-grants/{user_code}`, `public-shares/{token}/access`); this contract follows the owners, as 11 §8 says.
10. Request id: 11 (§5 layer 3, IT-11-4) issues a fresh UUIDv7 and does not echo a client `X-Request-Id`; `sc_error::CorrelationId::from_request` keeps a valid incoming value, `sc_observability` AC-3 assumes a caller-set id, and 16's `ApiClient` sends one. Owners of E-02, E-09 and E-21 to decide.

11. Closed 2026-11-08 (owner): `download_write_idle` (duration, default 60s, HTTP section of `sc_config`) is enforced on the connection socket for every response write, not per route.
12. Slow body at the idle timeout and handler over `request_timeout`: IT-11-13 allows "408 or reset"; `problem_details.md` lists no 408 and no code for a request timeout, so both end as a reset. Owners of E-21 to decide whether a status and a code are wanted.
13. Closed 2026-11-08 (owner): `sc_error` has `length_required`, 411, "Length required"; a block PUT without `Content-Length` gets it.
14. CSRF and `ShareGrant`: the grant is a path-scoped cookie resolved by 06 in the handler, so layer 9 does not check `ShareGrant` routes. If a share route with a grant cookie ever mutates state, owners of E-21 and 06 must decide whether it falls under the rule.
15. CSRF and `Sec-Fetch-Site`/`Origin` together: the rule is an "or" (either proof passes), so `Sec-Fetch-Site: cross-site` with an `Origin` equal to `public_url` passes. A browser never sends that pair; owners of E-21 to say if it should be refused.
16. Whether the web app's refresh request is cookie-authenticated, and so under CSRF, is not stated by 01 (E-15 owner). E-21/c12 registers `POST /auth/refresh-tokens` as `Public` with the refresh token in the body, so no cookie authenticates it and CSRF does not apply (Open point 38).
17. Wire of the test clock: only the request body `{"advance_seconds": N}` is fixed by FT-N-35. The offset answer `{"offset_seconds": N}` (whole seconds, the unit of the advance) is E-21/c15's choice; the supervisor poll (E-20/c09) must read it. `sc_clock` is non-negative and grow-only, while 11 section 4 says the endpoint "sets" the offset; c15 follows `sc_clock`.
18. The version gate runs before auth on every class but `Public`, `Worker` included. With `min_client_version` set, a worker call without `X-Sunnycloud-Client` is 400, and one below the minimum is 426, before the worker secret is checked, so the uniform 403 of AC-79 holds only for requests that pass the gate. Owners of E-21 and E-20 to say whether the supervisor sends the header or the gate should skip `Worker`.
19. `WorkerOutput` has `request_timeout` as its total handler time (table above), so a 10 GiB `transcode` output must arrive within 60 s. Owners of E-21 and E-18 to say whether it should follow `Block` and `Staged` (body idle timer, no total cap).
20. A route declares one success status. `claims` (200 or 204) and `outputs` (204 or 200) document the status of their first-named outcome in the OpenAPI document; the other is only in the table above.
21. `X-Worker-Id` and the failure `code` are passed to 08 as received (non-empty `X-Worker-Id`; no length or character rule); owner 08 to say whether either needs one. A missing or non-numeric header and a `job_id` that is not a UUID are 400; 08 section 5 lists no status for them.
22. Wire of the share routes (06 fixes the ports, not the bodies): create `{node_id, expires_in?, password?}`, answers and listing as in "Sharing and media routes"; `expires_in` is whole seconds; the listing is `{items: [...]}`; timestamps are RFC 3339 UTC with microseconds and `Z` (the form of the log lines). 11 holds its own copy of that formatter because `sc_observability`'s is crate-private; owners of E-02 and E-09 may export one from `sc_clock`. The route 11 §8 calls `.../access` is `.../password-verifications` (as 06 and open point 9 say) and answers 204 with the grant in the cookie only, no body.
23. Grant cookie, left open by 06 and 11: the name is `__Secure-share-grant` (`__Host-` needs `Path=/`, which would send the grant to every route); the attributes are `HttpOnly`, `Secure`, `SameSite=Lax` (Strict would drop it on a first navigation from an email or chat link), `Path=` the link's own path, `Max-Age` the remaining life of the grant, no `Domain`; the value is `<expires_at unix microseconds>.<value>` because `PublicShareService` takes the full `AccessGrant`. Owners of E-16 and E-21 to confirm; a change of name or attributes is a change of this contract.
24. CORS on share content: only `Access-Control-Allow-Origin: *` is added (200, 206, 304 of `GET` and `HEAD`); no `Access-Control-Expose-Headers`, so a page of another origin cannot read `Content-Range`, `ETag` or `Content-Length`, and no preflight answer (an `OPTIONS` is 405 with no CORS header). A cross-origin cookie is never sent, so a password link cannot be read from another origin. Owners to say if exposed headers are wanted.
25. `Range` as 11 reads it (`content_serving_headers.md`, request side): `DownloadOptions.range` is one `ByteRange{start, end}`, which cannot hold a suffix range (`bytes=-n`) or an open end. 11 maps `bytes=a-b` as is and `bytes=a-` to `end = u64::MAX` (03 must clamp an end past the size to the last byte, as RFC 9110 does; 03's contract says only that out of bounds is `RangeNotSatisfiable`), and ignores a suffix range, several ranges, another unit and a range that ends before it starts, so 200 with the whole content is served, which RFC 9110 allows. Resumable clients that send `bytes=N-` depend on 03 clamping. Owners of E-14 and E-21: extend `ByteRange` or the options with the open and suffix forms, or confirm the clamp.
26. The type of a derived blob: `BlobRef` carries none and `DerivedOpener` returns none, while `open_blob_content` takes `media_type` and 03 decides inline from it. 11 passes `image/webp` for `Thumb` and `Preview` and `video/mp4` for `Transcode` (08 section 7: WebP stills, MP4 transcodes). If 08's sniffer also accepts JPEG for stills the header would be wrong for those files; `DerivedOpener` should return the stored type with the `BlobRef` (contracts part 3 open point 6, 08 `derived_access.md` open point 3). Owners of E-00 and E-18.
27. `HEAD` on share content is never a download start (`starts_download` false), so a probe writes no `share_accessed` event; the table of 06 says "no `Range` header or a range starting at 0" and does not name `HEAD`. Owners of E-16 and E-21 to confirm.
28. 06 sets no rate limit on `GET .../content` beyond "shared with above"; 11 maps `RateLimited` to 429 with `Retry-After` on every visitor route and adds no limit of its own.
29. Wire of the tree, transfer and feed routes (02, 03 and 05 fix the ports, not the bodies): the names above follow the DTOs of `sync_core_proto.md` where it defines them (`node_id`, `current_revision`, `content_hash`, `next`, `outcome`, `lane_limit`, `entries`, `status`, `cursor`) and add what the ports carry beyond them. Where the two differ, the client side must change or the owners decide: `sync_core_proto.md` `BatchEntry` has `parent` and `file_hash` where 03's entry has `parent_id` and no hash, and `import_hint` as a JSON value where the port has a boolean (11 follows 03; an unknown member is 400, so a client that sends `file_hash` is refused); `UploadSession` for `get_upload` wants `lane_limit`, which `UploadState` does not carry; `RevisionRef` wants `size`, which `RevisionMatch` does not carry (11 answers `{revision_id, node_id, content_hash, client_meta}`; adding `size` needs a field in `contracts`); `ListingItem` is a `NodeView` with more members. 02's text calls the lookup body `{hashes}` and its answer members `hash`; 11 uses `content_hashes` (as `thumbnail-states` and the client do) and `content_hash` (as the client does). Owners of E-13, E-14, E-21 and E-23.
30. `thumb_state` of a listing item (02 section 3, `tree_reader.md`) is not on the wire: `NodeView` has no such field and the one port that knows thumbnail states takes content hashes (open point 6). Owners of E-13 and E-18 add the field to `NodeView` (a `contracts` change) or the client asks `thumbnail-states` for the hashes of a page.
31. `GET /revisions?client_device=` is paged by `offset` and `limit` (the port takes `Page{offset, limit}`) although 02 says keyset paging; the default limit 1,000 is 02's `listing_default_limit`, repeated in 11 because `list_children` and `find_revisions` take a number. A `PATCH /nodes/{id}` with neither member is passed to 02 as it is. Owners of E-13.
32. A feed cursor that is not `E.S` is answered by 11 as 422 `invalid_cursor` because the port takes a parsed `Cursor`; the same text as 05's `BadCursor`. `wait` is read in whole seconds (a fraction or a unit is 400) and passed unchanged: 05 caps it at `longpoll_wait_max`, 11 only bounds the request clock. Owners of E-12 and E-21.
33. `client_mtime` of a commit is `YYYY-MM-DDTHH:MM:SS[.f]Z` (UTC only); `sync_core_proto.md` has no `client_mtime` on a commit, so a client that has the file time sends it inside `client_meta` or in this member. A `client_meta` that is not JSON text coming back from 02 is answered as an opaque 500 and logged. Owners of E-13, E-14 and E-23.
34. `complete_link`: 01's route table lists no route for it (and `SignInService::complete_link` has no row in this table). Not registered by E-21/c12, so a link started by `POST /identity-link-attempts` cannot complete over HTTP: the callback URL of the provider is the one sign-in callback, whose `complete` finds no state of a link attempt. Owner of E-15 to say whether the callback chooses `complete_link` for a request that carries a session, or a second route exists.
35. Closed in 1.7.0: `GET /device-grants?user_code=` is registered; `sc_ids::UserCode::from_presented` owns the normalisation (case, hyphens and spaces ignored). A code that cannot exist is refused at the door with 400 and never reaches 01.
36. Cookie attributes: 01's DTOs carry the token (`SessionTokens.access`, `BrowserBinding`) and a lifetime, and 01's scope excludes cookie attributes, while 11 §8 says "set from the DTO 01 returns". 11 renders `<name>=<value>; Secure; HttpOnly; SameSite=Lax; Path=/` and, for the session cookie, `Max-Age=<expires_in>`; nothing else (no `Domain`, no `Expires`). The binding cookie has no lifetime (a browser-session cookie) because 01 reports none. A logout does not clear the session cookie (`Max-Age=0`): the revoked cookie answers 401. Owners of E-15 and E-21 to confirm, or to give the DTO attributes.
37. `GET /accounts/me` and `GET /accounts/me/quota` are `SessionOrDevice` (07's scope; 11 §8 says session only); the deletion is `SessionOnly`. Owners of E-17 and E-21.
38. Delivery of tokens (not stated by 01): the mobile code exchange and the refresh answer the tokens in the body for every client; the callback of a web sign-in sets only the session cookie (the access token), so a browser has no refresh token and its session lasts as long as the access token the port reports. If the web app should refresh, 01 and 11 must name a refresh cookie (name, `Path`, `Max-Age`). Owners of E-15 and E-26.
39. The SPA error route and its reasons are not named by 01 or 11: E-21/c12 redirects to `/login?error=<reason>` with `invalid_request` for `UntrustedState`, the `sc_error` code of `TrustedFailure{code}` and `invitation_required` for `InvitationRequired` (the "Not invited" state of the sign-in page); a `MobileCode` outcome redirects to `return_to` with `code=<code>` added to its query before any fragment (the mobile app reads it from its own deep link). Other refusals of the callback (`ProviderUnavailable`, `RateLimited`) stay the port's mapped answer, so a browser sees a problem body for them. Owners of E-15 and E-26 (AZ-3936).
40. Status of creations: the identity-link attempt, the device-grant start and the invite creation answer 200 (the task allows 201 only for the login attempt); the admin user list defaults to offset 0 and limit 1,000 (02's default, repeated because `list_users` takes a number) and passes a larger `limit` unchanged. `PATCH /admin/users/{id}` with neither member is passed to 07 as it is. Owners of E-17.
41. Device-poll answers, the invalid mobile code and `InvitationRequired` have no `sc_error` row: see `problem_details.md` Open point 3. The device poll answers 400 with `code` and `error` equal; a client that follows RFC 8628 reads `error`, a client of the problem body reads `code`. Owners of E-02 and E-15.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
| 1.1.0 | 2026-11-08 | Body limits and time limits enforced (E-21/c05): enforcement rules, `Timing` clocks, worker output caps, Open points 11 to 13 | AZ-3803 |
| 1.1.1 | 2026-11-08 | 411 `length_required` and the connection write-idle timer enforced; Open points 11 and 13 closed | AZ-3803 |
| 1.2.0 | 2026-11-08 | CSRF origin check enforced (E-21/c08): enforcement rules in the CSRF row, Open points 14 to 16, point 3 narrowed | AZ-3808 |
| 1.3.0 | 2026-11-11 | Worker routes and test-clock endpoints implemented (E-21/c15): section "Worker routes and the test clock", wire of the clock answers, Open point 7 narrowed, Open points 17 to 21 | AZ-3831 |
| 1.4.0 | 2026-11-11 | Sharing and media routes registered (E-21/c14): section "Sharing and media routes", the CORS row narrowed to the bytes of share content, `thumbnail-states` takes content hashes, the share thumbnail route is not registered (Open point 5), Open points 6 and 22 to 28 | AZ-3830 |
| 1.4.1 | 2026-11-11 | Share thumbnail route registered; Open point 5 closed except 06's missing route-table row (`authorize_derived` returns `ShareDerived`) | owner-approved change |
| 1.5.0 | 2026-11-11 | Tree, transfer and sync routes registered (E-21/c13): section "Tree, transfer and sync routes", `revisions:lookup` in class `Query` (Open point 4 closed), wire of the data path and its differences from `sync_core_proto.md`, Open points 29 to 33 | AZ-3829 |
| 1.6.0 | 2026-11-11 | Identity, account and admin routes registered (E-21/c12): section "Identity, account and admin routes", the callback is the one registered state-changing GET, `DELETE /devices/me` stays `DeviceOnly`, Open points 2, 3, 8 and 16 narrowed, Open points 34 to 41 (`complete_link` and the approval lookup have no route) | AZ-3828 |
| 1.7.0 | 2026-11-11 | The approval lookup `GET /device-grants?user_code=` registered; `sc_ids::UserCode::from_presented` added; Open point 35 closed | AZ-3828 |
| 1.8.0 | 2026-11-12 | The device grant answer carries `verification_uri` (`<public_url>/device`) next to `expires_in`; sync-core decodes the grant and poll answers as stated here | AZ-4231 |
