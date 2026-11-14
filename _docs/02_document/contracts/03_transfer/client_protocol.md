# Transfer client protocol: error catalogue, retry rules, one API process

**Component**: 03_transfer
**Producer task**: E-14/c16 — `_docs/02_tasks/todo/AZ-3762_transfer_docs.md`
**Consumer tasks**: E-23 (desktop), E-26 (web), E-27 (Android), later iOS; E-21/c11 and E-21/c13 (OpenAPI and handlers in 11); operators of a v1.0 installation
**Version**: 1.0.0
**Status**: draft (frozen when the producer task passes review)
**Last Updated**: 2026-11-14

## Purpose

One place that says, for every client of the transfer API, what each transfer error means and what to do next, and the retry rules for sessions, blocks, batches and conflict names. The behaviour behind it is frozen in `upload_service.md`, `commit_service.md` and `download_service.md`; the HTTP form (problem+json, `Retry-After`) is `../11_http_api/problem_details.md`. Every code and status below is taken from the code: `TransferError` (`src/components/03_transfer/src/error.rs`) and the single code table of `sc_error` (`src/shared/error/src/code.rs`); 11 maps the port errors in `src/components/11_http_api/src/port_error/files.rs`. Codes are stable; changes are additions only (OpenAPI additive-only diff, E-21/c11).

## 1. Error catalogue

One row per `TransferError` variant, each code once. The comparison with the error table runs in CI (`tests/transfer/errors.rs`, AZ-3762 AC-1); it reads the rows between the two markers.

<!-- transfer-error-catalogue:start -->
| Code | Status | Routes | Meaning | Client action |
|---|---|---|---|---|
| `quota_exceeded` | 507 | start, staged | The declared size does not fit the account's free quota (reserved bytes count) | Do not retry on a timer. Tell the user; retry only after the quota changed (files deleted, quota raised) |
| `free_space_floor` | 507 | start, block, staged | The server volume is below its free-space floor (also a full volume on a block write) | Do not retry on a timer. Report a server-side condition; retry later, after the operator freed space |
| `size_too_large` | 413 | start | The declared size is above `max_declared_size` (50 GiB) | Never retry; the file cannot be uploaded |
| `session_cap_exceeded` | 409 | start | The account already has `max_open_sessions` (8) open large uploads | Finish or abort an open session (`DELETE /api/v1/uploads/{upload_id}`), then start again |
| `staged_cap_reached` | 409 | staged | The account holds `max_staged` (1000) staged files not yet committed | Commit the staged files (`POST /api/v1/commits:batch`), then stage more; keep at most half the cap staged per client |
| `commit_id_reused` | 422 | batch | The `commit_id` arrived before with a different request (entries, order or any field) | A client bug: a persisted batch is resent unchanged. Never change entries under a `commit_id`; failed entries go into a new batch with a new `commit_id` |
| `batch_too_large` | 422 | batch | More entries than `batch_max_entries` (500, read from server info) | Split into batches of at most `batch_max_entries`, each with its own `commit_id`, persisted before the first send |
| `blocklist_mismatch` | 422 | commit, batch entry | The block list differs from the session's received hashes (count or a hash) | Hash the file again; if it changed, start a new session under a new `client_key` (section 2.1); do not resend the same list |
| `account_not_active` | 403 | every transfer route | The account is being deleted or is disabled | Stop every transfer of the account; do not retry |
| `parent_not_found` | 404 | start, commit, batch entry | The parent folder id is unknown in the account | Re-read the tree (feed), pick an existing folder, then start or commit again |
| `not_a_folder` | 422 | start, commit, batch entry | The parent id names a file | Re-read the tree, pick a folder, then start or commit again |
| `parent_deleted` | 410 | commit, batch entry | The parent of a new file was deleted (tombstoned) after the start; 03's `ParentGone` | Re-read the tree; commit the same upload (the same `upload_id` or a new batch entry) under a live folder. The session stays open until its TTL |
| `block_hash_mismatch` | 422 | block, staged | The body does not hash to `X-Blake3`, or bytes came without a hash on the staged route; nothing was written | Read the block from the file again and resend it once; a second mismatch means the file changed: new `client_key`, new session |
| `block_length_mismatch` | 422 | block, staged | `Content-Length` is not `min(2 MiB, remaining)` for the index, the body ended early, or a staged body is over 2 MiB | Resend with the exact length; a file over 2 MiB takes the large path |
| `block_conflict` | 409 | block | Another hash for an index already received or in flight: the file changed during the upload | Stop this session; start a new one under a new `client_key` (the old one expires or is aborted) |
| `need_blocks` | 409 | commit | The session still lacks blocks; `need_blocks` lists their indexes | Send exactly the listed blocks, then commit again with the same request |
| `content_unavailable` | 404 | content | The revision exists but its blob file is missing or corrupt | Do not retry; report it. The file needs a new upload from a client that still has it |
| `range_not_satisfiable` | 416 | content | The range starts at or past the end of the content (`Content-Range: bytes */<size>`) | Request again within the size, or without `Range` |
| `upload_session_gone` | 410 | upload state, abort, block, commit, batch entry | The `upload_id` is unknown, expired, aborted, committed or another account's; in a batch, the staged file is unknown or expired | Large path: start again with the same `client_key` (resume, or `NothingToSend` when the earlier commit went through). Staged path: stage the file again under its `staged_key` and commit it in a new batch |
| `overloaded` | 503 | block, staged | An in-flight cap (lane, account or buffer) or the same block already in flight; 0 body bytes were read | Wait `Retry-After` seconds when the header is present, then resend the same request unchanged |
<!-- transfer-error-catalogue:end -->

Other codes a transfer route answers, from the shared table and not from `TransferError`:

| Code | Status | Routes | Meaning | Client action |
|---|---|---|---|---|
| `invalid_name` | 422 | start, commit, batch entry | The name breaks the name rules | Never retry the same name; the user renames |
| `invalid_request` | 400 | every route | A malformed request: field types, a block index out of range, a `client_key` outside 1 to 256 bytes, a non-positive large declared size | A client bug; do not retry unchanged |
| `length_required` | 411 | block, staged | No `Content-Length` (chunked body) | Send the exact length |
| `not_found` | 404 | content | The node or revision is unknown, another account's, or not current for a user | Re-read the tree |
| `internal` | 500 | every route | An unexpected failure (logged with the request id) | Retry later with back-off; report the request id if it persists |

## 2. Client retry rules

### 2.1 Sessions and `client_key` (AC-50; IT-03-1, IT-03-4, FT-P-67)

- The `client_key` names one version of one source file, at most 256 bytes, unique per account: web `web:` + SHA-256 of parent id, name, size and `lastModified`; desktop `desk:` + volume id, file id, size and mtime; Android and iOS `asset:` + device id, MediaStore or Photos asset id and the file hash.
- The same key resumes the same session: `POST /api/v1/uploads` answers the existing `upload_id` and its `need_blocks`. A file that changed on disk gets a new key, so it starts a fresh session and never mixes blocks of two versions (AC-50).
- After a dropped connection, a closed tab or a server restart, start again with the same key (or `GET /api/v1/uploads/{upload_id}`) and send only `need_blocks`. Blocks acknowledged after the server's last checkpoint are listed again; the bound is 16 MiB re-sent per resume (AC-3, AZ-3731; IT-03-4).
- An open session lives `upload_session_ttl` (24 h) from its last checkpoint; past it the session is gone (`upload_session_gone`) and the same key starts anew.
- `NothingToSend{revision}` from a start is a success: the content is already the file's current revision, or the key's earlier commit created `revision` within `upload_session_ttl` (a lost commit response). Treat the upload as committed with that revision; nothing is sent (E-14/c02, AZ-3732; `upload_service.md` § `NothingToSend` rule). This heals a lost commit response on the large path, which has no receipt.

### 2.2 Blocks

- Each block is one `PUT /api/v1/uploads/{upload_id}/blocks/{index}` of exactly `min(2 MiB, remaining)` bytes with `X-Blake3`; 204 means the block is durable. At most `lane_limit` (4) blocks in flight per session.
- `overloaded` 503: wait `Retry-After` when present and resend the same block; the server read nothing of the body.
- `block_hash_mismatch` or `block_length_mismatch`: resend that block once from the file; repeated failure means the file changed (new key).
- Resending a block that was already acknowledged with the same hash is answered 204 without reading it, so a retry after a lost 204 is safe.

### 2.3 Commits and conflict names (E-14/c01, AZ-3731; IT-03-7)

- `POST /api/v1/commits` with the block list. `need_blocks` 409: send the listed blocks and commit again.
- An existing file is addressed by `base_revision`, never by its folder: a commit after the folder moved lands on the same file (AC-70). A stale `base_revision` is not an error: the answer is `Committed` with `conflict_copy` = the sibling that keeps the older content.
- Names are assigned by the server: when the name is taken, the new file is saved under a conflict-copy name and `conflict_copy` equals `node_id`. Commit never answers `name_conflict` or `edited_since`; never retry with another name. On any `conflict_copy`, read the folder again to learn the names.
- `parent_deleted` 410: the folder is gone; commit the same upload under a live folder.

### 2.4 Batches (FT-N-38, FT-P-04)

- Before the first send, persist the batch `commit_id` together with its `staged_key`s and entries. Reuse all of it unchanged on every retry: the request hash is then identical and the stored receipt answers with the first result (also after a lost response). A different request under the same id is `commit_id_reused`.
- The 200 answer carries one result per entry. Entries that failed are re-sent in a new batch with a new `commit_id`, after the cause is fixed (stage again on `upload_session_gone`, choose a live folder on `parent_deleted` or `parent_not_found`).
- A receipt lives `commit_receipt_ttl` (48 h); after it the same `commit_id` is a new batch, so a client does not retry a batch older than that: it lists the folder instead.
- At most `batch_max_entries` entries per batch; a staged file is committed within `upload_session_ttl` of its staging.

### 2.5 Downloads

- `416` with `Content-Range: bytes */<size>`: request within the size. `content_unavailable`: do not retry. `If-None-Match` with the ETag gives 304.

## 3. Operators: exactly one API process (decision 12, R06)

Upload progress (which blocks of each open session arrived) is held in the memory of the API process; the database holds only checkpoints. All lanes of one session must therefore reach the same process, so v1.0 runs exactly one API process and replicas are unsupported for uploads. Never scale the `api` service of the Compose file and never put two API processes behind one proxy. A restart is safe: clients resume from the last checkpoint (section 2.1). Stated also in `../../deployment/deployment_procedures.md` and `../../deployment/containerization.md`; the Compose file sets no `replicas` for the API.

## 4. Codes per route (AZ-3762 AC-2)

The codes each transfer route can return, from the contracts, compared with the `errors` lists of 11's route registry (`src/components/11_http_api/src/registry/transfer/mod.rs`), from which the generated OpenAPI document lists them:

| Route | Codes from the contracts | Missing from the registry list (E-21) |
|---|---|---|
| `POST /api/v1/uploads` | `invalid_name`, `parent_not_found`, `not_a_folder`, `size_too_large`, `quota_exceeded`, `session_cap_exceeded`, `free_space_floor`, `account_not_active` | none (`ClientKeyMismatch` has no wire code: see Open items) |
| `GET /api/v1/uploads/{upload_id}` | `upload_session_gone` | none |
| `DELETE /api/v1/uploads/{upload_id}` | `upload_session_gone`, `account_not_active` | `account_not_active` |
| `PUT /api/v1/uploads/{upload_id}/blocks/{index}` | `upload_session_gone`, `invalid_request` (index), `length_required`, `block_length_mismatch`, `block_hash_mismatch`, `block_conflict`, `overloaded`, `free_space_floor`, `account_not_active` (checkpoint) | `invalid_request`, `account_not_active` |
| `PUT /api/v1/staged-uploads/{staged_key}` | `quota_exceeded`, `staged_cap_reached`, `block_hash_mismatch`, `block_length_mismatch`, `length_required`, `free_space_floor`, `overloaded`, `account_not_active` | none (`ClientKeyMismatch`: see Open items) |
| `POST /api/v1/commits` | `need_blocks`, `blocklist_mismatch`, `upload_session_gone`, `invalid_name`, `parent_deleted`, `parent_not_found`, `not_a_folder`, `account_not_active` | none |
| `POST /api/v1/commits:batch` | `batch_too_large`, `commit_id_reused`, `account_not_active`; per entry in the 200 body: `invalid_name`, `parent_deleted`, `parent_not_found`, `not_a_folder`, `blocklist_mismatch`, `upload_session_gone`, and the code a failed effect carries (`not_found` when the base revision's node vanished, `internal`) | none |
| `GET|HEAD /api/v1/nodes/{node_id}/content`, `/api/v1/revisions/{revision_id}/content` | `invalid_request`, `not_found`, `range_not_satisfiable`, `content_unavailable` | none |

Every status in the registry comes from the same `sc_error` table as the catalogue, so a listed code always carries the catalogue's status.

## Open items

- `UploadError::ClientKeyMismatch` (same `client_key` or `staged_key` with another declared size or content) has no wire code: 11 answers it as `internal` 500 (`no_wire_code`). A client that derives keys per section 2.1 never sends it; a dedicated code is an E-21/E-14 owner decision (reported by AZ-3762).
- `UploadError::Overloaded` carries no delay, so 11 sends `overloaded` 503 without `Retry-After` on the block and staged routes, although `problem_details.md` puts `Retry-After` on every 503 (`TransferError::Overloaded` has `retry_after`, the port error has not). Until a delay is added (E-14 contracts change or a default in 11), clients back off on their own schedule (reported by AZ-3762).
- The registry lists of `DELETE /api/v1/uploads/{upload_id}` and the block route miss the codes of section 4 (E-21/c13).
- The release Compose file does not exist yet (only `deploy/compose/compose.dev.yml`, which sets no `replicas`); the check of AZ-3762 AC-3 is repeated when E-00 adds it (reported to E-00).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-14 | Initial: catalogue of the 20 transfer codes, client retry rules, one-process note, codes per route | AZ-3762 |
