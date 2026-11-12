# Contract: sync_core_proto — sans-IO protocol types and rules (`sync-core-proto`)

**Component**: 13_sync_core (crate `sync-core-proto`; builds native and `wasm32-unknown-unknown`)
**Producer task**: E-23/c04 — `_docs/02_tasks/todo/B09-04_sync_core_proto.md` (contributor: E-23/c05 `UploadPlanner`, `_docs/02_tasks/todo/B09-05_upload_planner.md`); keys assigned at Step 4
**Consumer tasks**: this batch: E-23/c05..c18 (every `sync-core` and `sync-core-wasm` task); other batches through `sync-core`, `sync-core-wasm` or `mobile_backup`: E-24 (14 daemon), E-26 (16 web Worker, through the WASM exports), E-27 and E-28 (`mobile_backup`); resolved at Step 4
**Version**: 1.1.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The one sans-IO definition of the client side of the wire protocol: the DTOs both executors (reqwest in `sync-core`, `fetch` in the browser Worker) send and receive, and the pure rules that must match the server byte for byte (hashing wrappers, commit-ID rules, feed-page checks, the upload planner, server URL validation). No tokio, no HTTP, no clock, no randomness of its own.

Sources: `components/13_sync_core/description.md` §1, §2, §3, §7; `module-layout.md` (crate `sync-core-proto`); `contracts/11_http_api/server_info.md`, `api_v1_surface.md`, `problem_details.md`; `contracts/05_sync_feed/change-feed.md`; `contracts/03_transfer/commit_service.md`; `contracts/shared_ids_hashing/sc_ids.md`; ADR-004, ADR-015.

## Shape

### Constants

| Name | Value | Note |
|---|---|---|
| `BLOCK_SIZE` | 2 MiB (re-exported from `sc_ids`) | a server reporting another value is a protocol mismatch |
| `MAX_CLIENT_LANES` | 4 | effective lanes = `min(4, ServerInfo.max_lanes)` |
| `LOOKUP_CHUNK` | 1000 | maximum hashes per `revisions:lookup` request |

### DTOs (wire shapes; JSON field names as in the OpenAPI document of 11, which is the authority)

| Type | Fields | Note |
|---|---|---|
| `ServerInfo` | `block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, `longpoll_max_wait`, `min_client_version`, `api_versions` | `server_info.md`; unknown fields ignored |
| `Cursor` | `epoch: u64`, `seq: u64` | wire text `E.S`; total order by `(epoch, seq)` |
| `ChangeItem` | `node_id`, `parent_id`, `kind`, `name`, `state` (`live` \| `tombstone`), `revision_id?`, `size?`, `content_hash?`, `seq` | latest node state, not an event (13 risk 5) |
| `ChangePage` | `items: [ChangeItem]`, `next_cursor: Cursor`, `has_more: bool` | |
| `ResyncReason` | `EpochChanged`, `BelowPurgeHorizon`, `CursorAhead`, `InvalidCursor` | first three from the 410 `reason`; `InvalidCursor` from 422 `invalid_cursor` |
| `NodeView` | `node_id`, `parent_id?`, `kind`, `name`, `current_revision?` | |
| `ListingItem` | node fields plus `content_hash` for files (none for folders) | AC-31 |
| `ListingPage` | `items: [ListingItem]`, `next: Option<PageToken>` | |
| `RevisionView` | `revision_id`, `blocklist: [Hash32]`, `content_hash: FileHash`, `size`, `client_meta?` | |
| `RevisionRef` | `revision_id`, `node_id`, `content_hash`, `size`, `client_meta?` | |
| `UploadSession` | `upload_id`, `need_blocks: [u32]`, `lane_limit`, `expires_at` | answer of `start_upload` and `get_upload` |
| `StartOutcome` | `Session(UploadSession)` \| `NothingToSend{revision}` | |
| `CommitOutcome` | `Published{node, revision}` \| `ConflictCopy{node, copy}` \| `NothingToSend{revision}` | built from 03's `Committed{node_id, revision_id, conflict_copy?}`; exact field use follows the G-03-3 decision (E-14/c01) |
| `Staged` | `staged_key: Uuid`, `size: u64` | answer of `stage_small`; added by AZ-3963 (owner approved 2026-11-06); the server-side `contracts::Staged` carries `size: i64` |
| `BatchEntry` | `staged_key`, `parent`, `name`, `base_revision?`, `file_hash`, `client_meta?`, `import_hint?` | |
| `BatchCommit` | `commit_id: Uuid`, `entries: [BatchEntry]` (<= `batch_max_entries`) | |
| `BatchResult` | per entry `Committed{node, revision, conflict_copy?}` \| `EntryError{code}` | |
| `DeviceAuthorization` | `device_code`, `user_code`, `verification_uri`, `interval`, `expires_in` (seconds) | as `POST /api/v1/device-grants` answers (AZ-4231) |
| `DevicePoll` | `Token(DeviceToken)` \| `Pending` \| `SlowDown` \| `Denied` \| `Expired` | built from a 200 `{access_token, token_type: "Bearer"}` or a 400 code (AZ-4231) |
| `DeviceToken` | `access_token`, `token_type` (`Bearer`) | a secret: never printed by `Debug` |
| `DeviceRefusal` | `authorization_pending`, `slow_down`, `access_denied`, `expired_token` (`from_code`) | the four 400 codes of the poll |
| `SessionTokens` | `access_token`, `refresh_token`, `expires_in` (seconds) | built from the 200 answer of `/auth/mobile-code-exchanges` and `/auth/refresh-tokens` (AZ-4236); token values are secrets: wrapped in `sync-core`, never `Debug`-printed |

### Functions and state machines

| Name | Signature (indicative) | Errors | Blocking? |
|---|---|---|---|
| `ServerUrl::parse` | `(text, allow_plain_http: bool) -> Result<ServerUrl, UrlError>` | `InsecureScheme` (scheme `http` and plain HTTP not allowed by the caller's build), `Invalid` (not an absolute URL with a host, or another scheme) | sync, pure |
| hashing wrappers | `hash_block`, `FileHasher`, `rollup` re-exported from `sc_ids`; `ResumeHashState{blocks_done, block_hashes}` so hashing resumes at a block boundary | `BlockTooLarge` | sync, pure |
| `client_key_for_mobile` | `(device_id, asset_id, file_hash) -> Uuid` = `sc_ids::derive_client_key` | none | sync, pure |
| `reuse_or_new_key` | `(stored: Option<(Uuid, FileHash)>, current: FileHash, fresh: impl FnOnce() -> Uuid) -> Uuid` — the stored key when its hash equals `current`, else `fresh()` | none | sync, pure |
| `check_batch_ids` | `(commit_id, member_keys) -> Result<(), BatchIdError>` — `commit_id` is a UUIDv7 and equals no member key | `NotV7`, `EqualsMemberKey` | sync, pure |
| `check_page` | `(cursor: Cursor, page: &ChangePage) -> Result<(), PageViolation{rule, detail}>` | rules below | sync, pure |
| `UploadPlanner` (E-23/c05) | `new(block_count, need_blocks, lanes_max, retry_policy)`; `next_actions() -> [Action]`; `on_result(i, BlockResult)`; `on_session(need_blocks)`; `on_commit(CommitResult)` | none (state machine) | sync, pure |

`Action` = `SendBlock{i}` \| `Wait{ms}` \| `RereadSession` \| `Commit` \| `Done` \| `RestartSession`.
`BlockResult` = `Acked` (204 only) \| `HashRejected` (422 `block_hash_mismatch`) \| `Transient{retry_after?}` \| `BlockConflict{retry_after?}` (409 `block_conflict`) \| `SessionGone` (410 `upload_session_gone`).
`CommitResult` = `NeedBlocks{list}` (409 `need_blocks`) \| `BlocklistMismatch` (422) \| `Ok` \| `SessionGone`.
`RetryPolicy` = `{base, max, max_attempts_per_block}` (from `sync.retry_base`, `sync.retry_max`, `sync.max_attempts_per_block`).

### Feed-page rules (`check_page`, AC-69 client side)

| Rule | Violation |
|---|---|
| P1 | items not strictly ascending by `(seq, node_id)` |
| P2 | an item with `seq <= cursor.seq` |
| P3 | an item with `seq > next_cursor.seq` |
| P4 | `next_cursor.epoch != cursor.epoch` |
| P5 | `next_cursor < cursor` |
| P6 | a duplicate `(seq, node_id)` |

## Invariants

- The crate has no tokio, HTTP, clock or OS-randomness dependency, and builds for `wasm32-unknown-unknown`; time and randomness enter only as arguments (`fresh`, `retry_after`).
- Hash, rollup and mobile-key results equal `sc_ids` byte for byte on every target (vector file, `wasm_exports.md`).
- The planner never has more than `min(4, lanes_max)` blocks in flight, never re-sends an acked block unless `on_session`/`on_commit` re-lists it, resends a block after `HashRejected`, never emits `Commit` while a needed block is outstanding, and keeps the same `client_key` across `RestartSession` (the caller re-hashes first after `BlocklistMismatch`).
- `BlockConflict`: `Wait{retry_after}` then `RereadSession`; still needed → resend (counts as `Transient`); not needed and sent by this run → `Acked`; otherwise → `RestartSession`.
- `check_page` never repairs a page: any rule violation is an error naming the rule.
- No type here carries a token outside `SessionTokens` and `DeviceToken`, and no DTO field is a local path.

## Non-Goals

- Not covered: HTTP execution, retry timing with jitter, cancellation, token handling and host traits (`sync_api.md`, `host_traits.md`).
- Not covered: the WASM binding shape (`wasm_exports.md`) and UniFFI derives (`uniffi_surface.md`).
- Not covered: reconcile and merge rules (14), the mobile backup run (17), cursor persistence (always the caller's).

## Versioning Rules

- **Breaking**: a DTO field removed or retyped, a planner action or result removed, a page rule relaxed, a change of `BLOCK_SIZE`, `LOOKUP_CHUNK` or a hashing context (frozen for v1 by `sc_ids`).
- **Non-breaking**: a new optional DTO field, a new `ResyncReason` that callers treat as a resync.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| url-plain | `http://sunnycloud.test`, plain HTTP not allowed | `InsecureScheme` | IT-13-13, AC-54 |
| url-ok | `https://sunnycloud.test` | accepted | |
| page-rules | v1..v6 around cursor `1.100`; valid page 101..150, next `1.150` | P1..P6 each named; valid page passes | IT-13-11 |
| planner-lanes | random results, `lanes_max` in {1, 2, 4, 8} | in flight <= `min(4, lanes_max)` at every step | IT-13-1 |
| planner-reactions | cases (a)..(g) of IT-13-2 | the documented action per case | IT-13-2 |
| key-reuse | stored `(K, H)`, current `H`; then current `H'` | `K`; then a fresh key | IT-13-17 |
| mobile-key | `(dev-1, asset-42, H)` twice, and with `H'` | equal; different | IT-13-17 |
| batch-id | `commit_id` equal to a member key | `EqualsMemberKey` | IT-13-17 |

## Open points

1. `longpoll_max_wait` unit on the wire (seconds or milliseconds) is not specified (`server_info.md` open point 1).
2. The planner has no action for a block that exhausted `max_attempts_per_block` (IT-13-1 only says "the planner stops retrying it"); the terminal action and the error the executor returns are decided by E-23/c05.
3. `ServerUrl` rules beyond "https with a host" (user info, query, fragment, path handling) are not specified; E-23/c04 decides and records them here.
4. A pure UUIDv7 constructor from (time, random bytes) does not exist in `sc_ids` (`new_id` takes a `Clock`); the WASM "commit ID" export needs one or the Worker generates keys itself (`wasm_exports.md` open point 1).
5. `UploadSession.lane_limit` versus `ServerInfo.max_lanes`: which one bounds a session when they differ is not stated; this contract takes the lower of the two.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
| 1.1.0 | 2026-11-12 | `SessionTokens` field names follow the server (`access_token`, `refresh_token`, AZ-4236); `DeviceAuthorization.expires_in`; `DevicePoll` decoded from the server's 200 token or 400 code; `DeviceToken`, `DeviceRefusal` | AZ-4231 |
