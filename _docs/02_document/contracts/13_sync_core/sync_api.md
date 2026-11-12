# Contract: sync_api — `SyncApi`, `HttpSyncClient::connect`, typed errors and the wire-code table

**Component**: 13_sync_core (crate `sync-core`)
**Producer task**: E-23/c07 — `_docs/02_tasks/todo/B09-07_http_sync_client_connect.md` (trait, construction, negotiation, transport); contributors: E-23/c06 typed errors and wire table (`B09-06`), E-23/c08 tree operations (`B09-08`), E-23/c09a `hash_file` (`B09-09`), E-23/c09b `upload` (`B09-10`), E-23/c10 `stage_small`/`batch_commit` (`B09-11`), E-23/c11 `download` (`B09-12`), E-23/c12 feed (`B09-13`), E-23/c13 token lifecycle (`B09-14`), E-23/c14 `cancel`, rate limits, `SyncConfig` (`B09-15`); keys assigned at Step 4
**Consumer tasks**: this batch: E-23/c15, c18a, c18b; other batches: E-24 (14 daemon: device login, uploads, downloads, feed loop, first-sync merge), E-27 (`mobile_backup`: sign-in, refresh, uploads, reinstall rebuild), E-28 through `mobile_backup`; pipelines AZ-3668, AZ-3669, AZ-3670, AZ-3672, AZ-3673, AZ-3674; resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The typed client API hosts code against. `HttpSyncClient` is its one production implementation; host-side unit tests fake the trait (never 13's own tests). No HTTP type appears in a signature; every wire code becomes a typed error in one table. Operations land task by task; this file is the frozen target shape.

Sources: `components/13_sync_core/description.md` §2, §3, §5, §7, §11; `contracts/11_http_api/server_info.md`, `api_v1_surface.md`, `problem_details.md`, `content_serving_headers.md`; `contracts/shared_error/sc_error.md`; `contracts/03_transfer/upload_service.md`, `commit_service.md`; `contracts/05_sync_feed/change-feed.md`; `canonical-names.md` § Runtime.

## Shape

### Construction and negotiation

`HttpSyncClient::connect(SyncConfig, Arc<dyn CredentialProvider>) -> Result<(HttpSyncClient, ServerInfo), ConnectError>`

`ConnectError` = `InsecureScheme` \| `Unreachable` \| `UpgradeRequired{message, update_url}` \| `ProtocolMismatch{what}` \| `Unauthorized`.

| Step | Rule |
|---|---|
| 1 | `sync.server_url` parsed by `ServerUrl::parse`; `http://` is `InsecureScheme` with 0 sockets opened unless the build has the compile-time feature `allow-plain-http` (debug builds only) |
| 2 | `GET /api/v1/server-info` (exempt from the version gate) |
| 3 | `block_size != 2 MiB` → `ProtocolMismatch{what: block_size}` |
| 4 | `api_versions` without `v1`, or `min_client_version` above the client's own version → `UpgradeRequired` |
| 5 | every number range-checked; a value outside the accepted range is `ProtocolMismatch` or a documented clamp; lanes = `min(4, max_lanes)`; server limits win over local settings |
| 6 | every later request carries `X-Sunnycloud-Client: <family>/<semver>` (family `desktop`, `android`, `ios`) and `Accept` for `/api/v1`; a 426 anywhere is `UpgradeRequired` with the server text, never retried |

Transport: one HTTP/2 client per `HttpSyncClient`, TLS 1.3 only, ALPN h2, HTTPS only; explicit windows `sync.h2_stream_window` / `sync.h2_conn_window` (8 MiB / 32 MiB start values, tuned by gate S1); keep-alive pings every `sync.keepalive_interval` while idle; system trust store; no redirect followed to another host; release binaries are built without `allow-plain-http`.

### `SyncConfig` (typed section `sync`, rows in helper 01's key table; durations from config)

`server_url`, `client_family`, `client_version` (build-injected), `retry_base` 500ms, `retry_max` 30s, `max_attempts_per_block` 8, `connect_timeout` 10s, `request_idle_timeout` 60s, `longpoll_wait` 30s (clamped to `ServerInfo.longpoll_max_wait`), `longpoll_client_margin` 10s, `h2_stream_window` 8 MiB, `h2_conn_window` 32 MiB, `keepalive_interval` 20s, `hash_cpu_share` (desktop 0.25 of a core), `hash_checkpoint_blocks` 32, `batch_max_bytes_in_flight` 64 MiB. Environment names `SUNNYCLOUD_SYNC_<KEY>`.

### Operations (`SyncApi`, async)

| Operation | Route(s) (`api_v1_surface.md`) | Output | Typed errors beyond the common set | Contributor |
|---|---|---|---|---|
| `hash_file(source, resume?, progress)` | none | `HashedFile{block_hashes, file_hash}` | `SourceChanged`, `Io` | c09a |
| `lookup_revisions(hashes)` | `POST /api/v1/revisions:lookup`, chunks of 1000 | `[RevisionRef]` with `client_meta` (reinstall rebuild only) | none | c08 |
| `get_revision(id)`, `root_node()` | `GET /api/v1/revisions/{id}`, `GET /api/v1/nodes/root` | `RevisionView`; `NodeView` | `NotFound` | c08 |
| `list_children(node, page)` | `GET /api/v1/nodes/{id}/children` | `ListingPage` (files carry `content_hash`) | `NotFound`, `NotAFolder` | c08 |
| `create_folder`, `move_node`, `delete_node` | `POST /api/v1/nodes`, `PATCH /api/v1/nodes/{id}`, `DELETE /api/v1/nodes/{id}?base_revision=` | `NodeView` / `()` | `NameConflict`, `InvalidName`, `Cycle`, `ParentNotFound`, `ParentGone`, `NotAFolder`, `EditedSince{current}` (delete of a file only); `move_node` sends no base revision | c08 |
| `upload(request, checkpoints, progress)` | `POST /api/v1/uploads`, `GET /api/v1/uploads/{id}`, `PUT /api/v1/uploads/{id}/blocks/{index}` (`X-Blake3`, 204), `POST /api/v1/commits` | `CommitOutcome` | `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`, `InvalidName`, `NameConflict`, `EditedSince{current}`, `ParentNotFound`, `ParentGone`, `BlocklistMismatch`, `SourceChanged` | c09b |
| `stage_small(staged_key, bytes, hash)` | `PUT /api/v1/staged-uploads/{staged_key}` (`X-Blake3`) | `Staged{staged_key, size}` | `QuotaExceeded`, `StagedCapReached`, `FreeSpaceFloor` | c10 |
| `batch_commit(BatchCommit)` | `POST /api/v1/commits:batch` | `BatchResult` (stored response on a repeated id) | `BatchTooLarge`, `CommitIdReused` | c10 |
| `download(revision, sink, resume_from)` | `GET /api/v1/revisions/{id}/content` with `Range` | `Verified{file_hash}` | `HashMismatch{block}`, `NotFound`, `BlobUnavailable`, `Io` | c11 |
| `poll_changes(cursor, wait, limit)`, `head_cursor()` | `GET /api/v1/changes?cursor=&wait=&limit=`, `GET /api/v1/changes/head` | `ChangePage` or `ResyncRequired{reason}`; `Cursor` | `ProtocolViolation{detail}` | c12 |
| `device_grant_request`, `device_grant_poll` | `POST /api/v1/device-grants`, `POST /api/v1/device-tokens` | `DeviceAuthorization`; `DevicePoll` | none | c13 |
| `exchange_mobile_code(code, code_verifier)` | `POST /api/v1/auth/mobile-code-exchanges` | `SessionTokens` | `InvalidCode` | c13 |
| `refresh()` | `POST /api/v1/auth/refresh-tokens` | `SessionTokens` | `Unauthorized` | c13 |
| `sign_out()` | `DELETE /api/v1/devices/me` (13 §2; see open point 4) | `()` | none | c13 |
| `cancel()` | none | every in-flight operation ends with `Cancelled` within 1 s; nothing is sent afterwards | none | c14 |

Common errors on every operation: `Transport`, `Cancelled`, `UpgradeRequired`, `Unauthorized`, `CredentialUnavailable` (the host's credential could not be read; nothing was sent, AZ-4231), `RateLimited{retry_after}`, `AccountNotActive`, `Unexpected{code, request_id}`. `DeviceGrantRefused` is internal: the device poll answers it as an outcome.

### Wire-code table (the only place wire codes become typed errors; E-23/c06)

| Wire code (status, `sc_error`) | Typed | Caller action |
|---|---|---|
| `staged_cap_reached` (409) | `StagedCapReached` | commit what is staged, then stage again |
| `commit_id_reused` (422) | `CommitIdReused` | drop the batch, build a new one with a new id |
| `batch_too_large` (422 for more entries than `batch_max_entries`; a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer; resolved 2026-11-07) | `BatchTooLarge` | a bug: the executor already chunks |
| `size_too_large` (413) | `SizeTooLarge` | permanent for this file |
| `account_not_active` (403) | `AccountNotActive` | stop all work |
| `blocklist_mismatch` (422) | `BlocklistMismatch` | planner `RestartSession` after re-hashing |
| `parent_not_found` (404), `parent_deleted` (410) | `ParentNotFound`, `ParentGone` | re-list the parent |
| `not_a_folder` (422) | `NotAFolder` | re-list |
| `quota_exceeded` (507), `free_space_floor` (507) | `QuotaExceeded`, `FreeSpaceFloor` | pause with a visible message |
| `resync_required` (410, `reason`), `invalid_cursor` (422, feed only) | `ResyncRequired{reason}`, `ResyncRequired{InvalidCursor}` | head cursor first, then listing |
| `block_conflict` (409) | planner `BlockConflict{retry_after?}` | planner rules |
| `block_hash_mismatch` (422) | planner `HashRejected` | resend the block |
| `upload_session_gone` (410) | planner `SessionGone`; at commit, lost-commit recovery | restart with the same `client_key` |
| `need_blocks` (409, `need_blocks`) | planner `NeedBlocks{list}` | send the listed blocks, commit again |
| `name_conflict` (409), `edited_since` (409), `cycle` (409), `invalid_name` (422) | `NameConflict`, `EditedSince{current}`, `Cycle`, `InvalidName` | host decides |
| `session_cap_exceeded` (409) | `SessionCapReached` | wait for an open session to finish |
| `not_found` (404), `content_unavailable` (404) | `NotFound`, `BlobUnavailable` | |
| `unauthenticated` (401) | refresh path (session) or `Unauthorized` (device token) | token lifecycle |
| `rate_limited` (429, `Retry-After`) | `RateLimited{retry_after}` | next attempt not before `retry_after` |
| `upgrade_required` (426) | `UpgradeRequired{message, update_url}` | show the server text, no retry |
| any other code | `Unexpected{code, request_id}` | report |

## Invariants

- Exactly one mapping table; no operation turns a status or code into an error elsewhere.
- A block counts as done only on the server's 204; any other status, a reset or a body after a non-204 leaves it needed (AC-38, AC-68 client share).
- Stored `client_key` and batch `commit_id` are reused on every retry; a lost commit response is recovered by `start_upload` with the same key (`NothingToSend`), never by a new name (AC-78 contribution, ADR-005).
- Mutations carry the caller's `base_revision` (upload of an existing file, delete of a file); `move_node` carries none (AC-8, AC-70).
- The core never persists anything and never stores the feed cursor; hosts persist after applying.
- Tokens are absent from logs, error text and URLs; requests never go to a host other than `sync.server_url`.
- Memory per operation is bounded: at most `min(4, lanes)` block buffers of 2 MiB plus hash state; no whole-file buffer (AC-52).

## Non-Goals

- Not covered: protocol DTOs and pure rules (`sync_core_proto.md`), host traits (`host_traits.md`), FFI shapes (`uniffi_surface.md`), the WASM subset (`wasm_exports.md`).
- Not covered: reconcile/merge (14), the backup run (17), token storage (hosts), any server behaviour.

## Versioning Rules

- **Breaking**: an operation removed or its signature changed, a typed error removed or renamed, a wire-table row changed to another typed error.
- **Non-breaking**: a new operation, a new typed error for a new wire code (unknown codes already map to `Unexpected`).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| negotiate | `block_size` 1 MiB; `min_client_version` above; `api_versions` without v1; `max_lanes` 2 and 16 | `ProtocolMismatch`; `UpgradeRequired`; `UpgradeRequired`; lanes 2 and 4 | IT-13-13 |
| plain-http | `http://` URL in a release-profile build | `InsecureScheme`, 0 sockets | IT-13-13, ST-13-1 |
| version-header | every request of a session | all but `server-info` carry `X-Sunnycloud-Client` | IT-13-13 |
| wire-table | every row above, plus `weird_code` with request id `r-1` | the row's typed error; `Unexpected{code: "weird_code", request_id: "r-1"}` | IT-13-13 |
| ack-204 | block PUT answered 200 with body, 202, 500, reset | block stays needed and is re-sent | IT-13-15 |
| cancel | 3 uploads in flight, `cancel()` | all `Cancelled` within 1 s, nothing sent after | IT-13-18 |
| rate-limit | 429 `Retry-After: 3` | next attempt not before 3 s, jitter under 1 s | IT-13-19 |

## Open points

1. `ApiError::parse` (`sc_error`) turns an unknown code into `internal`; IT-13-13 expects `Unexpected{code: "weird_code", ...}` with the raw code kept. E-23/c06 needs the raw code from the parser (E-02 owner).
2. `EditedSince{current}`: the problem body (`problem_details.md`, `sc_error` `BusinessParams`) has no field carrying the current revision id.
3. `InvalidCode` (reused or expired mobile code) and the RFC 8628 device-poll answers (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`) have no rows in the `sc_error` table yet (`sc_error.md` open point 1); E-15 adds them.
4. `sign_out` uses `DELETE /api/v1/devices/me`, which `api_v1_surface.md` declares `DeviceOnly`: a mobile session would get 403. The mobile sign-out route (for example `DELETE /api/v1/sessions/current`) needs an owner answer (E-15, E-21, E-27).
5. Handling of 500 and 503 outside block PUTs (retry with backoff, or surfaced) is not stated; IT-13-15 covers block PUTs only.
6. Gate S1 may require a second HTTP/2 connection (13 risk 1); the decision is E-23/c02's and would add a config key.
7. Commit answers for stale-base and taken-name commits (`ConflictCopy` fields, whether `EditedSince` occurs on commit) follow G-03-3 (E-14/c01).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
