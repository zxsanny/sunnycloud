# 13 Sync Core

## 1. High-Level Overview

**Purpose**: The shared Rust client library that speaks the sunnycloud sync protocol for the desktop daemon, Android and iOS: block hashing, the 2 MiB blocklist upload with up to four lanes and resume, single-request small upload with batch commit, verified download, the change-feed long-poll client, tree operations with base revisions, device-grant, mobile-code and token-refresh calls, server limits (`ServerInfo`) and API version negotiation. One responsibility: *be the only implementation of the client side of the wire protocol*. No UI, no filesystem watching, no scheduling, no local index.

**Architectural pattern**: Two parts, a sans-IO protocol part and a client part, plus a thin WASM adapter and feature-gated FFI derives.
- **Protocol part**, crate `sync-core-proto` (**sans-IO**, builds native and `wasm32-unknown-unknown`): protocol DTOs, `UploadPlanner` (pure state machine: which block next, retry/backoff decisions, resume diff from `need_blocks`), hashing wrappers over `sc_ids` (helper 05), commit-ID rules, feed-page invariant checks, `ServerUrl` validation. No tokio, no HTTP, no clock.
- **Client part**, crate `sync-core` (native only, tokio + reqwest, depends on the protocol part): the executor. Runs the planner's actions over HTTP/2, owns retry timing, cancellation and the typed client API below.
- `sync-core-wasm` (thin wasm-bindgen adapter, a build target of the protocol part, not a third part; built for 16_web_app): block hashing, rollup, commit ID, resume diff. The browser Worker drives `fetch` itself and calls this module for every rule that must match the server byte for byte; the web app carries no third protocol implementation.
- Feature `uniffi` only adds derives (`#[uniffi::Record]`, `#[uniffi::Error]`, foreign-trait callbacks) on the public types. **The mobile cdylib is assembled by 17_android_app** (`mobile_backup` + 13 + later 19, one library per ABI, UniFFI for Kotlin and Swift). Kotlin and Swift own the UniFFI handle and pass status to Flutter over a Method/EventChannel (17, 18); there is no flutter_rust_bridge. 13 does not own the checkpoint DB, the backup run or `AssetSource` (those are `mobile_backup`).

Justification for the protocol/client split (named requirement): two executors exist (reqwest here, `fetch` in the browser) and both must plan identically; a single sans-IO state machine is cheaper than two protocol implementations and keeps the deferred iOS system-upload option open without building it now.

**Upstream dependencies**: helper 05 `sc_ids` (block hash, `FileHasher`, `rollup`, `derive_client_key`), 02 `ApiError::parse`, 01 config (client settings), 06 `sc_names` (optional preflight), 11_http_api at runtime (OpenAPI is the authority).
**Downstream consumers**: 14_desktop_daemon, 17_android_app (`mobile_backup`), 18_ios_app, 16_web_app (WASM subset). No cycles: 13 imports no component crates and not the server-only `contracts` crate; the traits that hosts implement are defined in `sync-core` itself (section 2a).

**Scope in / out**: as above; out: reconcile/merge rules (14, F9), backup run and asset mapping (17), settings, token storage (hosts), vault crypto (19), server logic.

## 2. Internal Interfaces

### Interface: SyncApi (async, typed, no HTTP types in signatures)
`SyncApi` is the trait that hosts and tests code against. `HttpSyncClient` is its one production implementation (reqwest); tests fake the trait. Construction: `HttpSyncClient::connect(SyncConfig, Arc<dyn CredentialProvider>) -> Result<(HttpSyncClient, ServerInfo), ConnectError>`. It calls `GET /api/v1/server-info` first (exempt from the version gate) and sends the header `X-Sunnycloud-Client: <family>/<semver>` on every other request. `ConnectError`: `InsecureScheme`, `Unreachable`, `UpgradeRequired{message, update_url}`, `ProtocolMismatch{what}`, `Unauthorized`.

`ServerInfo{block_size, max_lanes, small_file_max, batch_max_entries, max_staged_per_account, longpoll_max_wait, min_client_version, api_versions}` is defined in 11 and consumed here at connect (section 3).

| Operation | Input | Output | Typed errors (beyond the common set below) |
|---|---|---|---|
| `hash_file` | `Arc<dyn FileSource>`, `Option<ResumeHashState>`, progress | `HashedFile{block_hashes, file_hash}`; throttled by `hash_cpu_share`, checkpoint every `hash_checkpoint_blocks` | `SourceChanged`, `Io` |
| `lookup_revisions` | `[FileHash]` (the executor chunks to 1000 per call) | `[RevisionRef]` for the hashes the account holds, with `client_meta` (`POST /api/v1/revisions:lookup`). Used only to rebuild a mobile checkpoint after reinstall (AC-46); never to skip bytes of an upload (cross-node dedupe is out of scope) | none |
| `get_revision` | `revision_id` | `RevisionView{blocklist, content_hash, size, client_meta}` | `NotFound` |
| `root_node` | none | `NodeView` of the account root | none |
| `upload` | `UploadRequest{parent, name, source, hashed, base_revision?, client_key, client_meta?, import_hint}`, `Arc<dyn CheckpointStore>`, progress | `CommitOutcome::{Published{node, revision}, ConflictCopy{node, copy}, NothingToSend{revision}}` | `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`, `InvalidName`, `NameConflict`, `EditedSince{current}`, `ParentNotFound`, `ParentGone`, `BlocklistMismatch`, `SourceChanged` |
| `stage_small` | `staged_key`, bytes <= `small_file_max`, hash (`PUT /api/v1/staged-uploads/{staged_key}`, header `X-Blake3`; idempotent by key) | `Staged{staged_key, size}` | `QuotaExceeded`, `StagedCapReached`, `FreeSpaceFloor` |
| `batch_commit` | `BatchCommit{commit_id, entries <= batch_max_entries}` | `BatchResult[per entry: Committed \| EntryError]` (stored response replayed on a repeated id) | `BatchTooLarge`, `CommitIdReused` |
| `download` | `RevisionRef`, `Arc<dyn DownloadSink>`, `resume_from` | `Verified{file_hash}` | `HashMismatch{block}`, `NotFound`, `BlobUnavailable`, `Io` |
| `poll_changes` | `Cursor{epoch, seq}`, `wait`, `limit` | `ChangePage{items, next_cursor, has_more}` or `ResyncRequired{reason}` (HTTP 410, or `invalid_cursor` 422 mapped to `ResyncRequired{InvalidCursor}`) | `ProtocolViolation{detail}` |
| `head_cursor`, `list_children` | account root or node, page token | `Cursor`; `ListingPage{items, next}` (each item carries `content_hash` for files) | `NotFound`, `NotAFolder` |
| `create_folder`, `move_node`, `delete_node` | ids, name; `base_revision` for `delete_node` of a file only (`move_node` has none: a move is by node id and never conflicts with an edit, AC-70) | `NodeView` / `()` | `NameConflict`, `InvalidName`, `Cycle`, `ParentNotFound`, `ParentGone`, `NotAFolder`, `EditedSince{current}` (delete only) |
| `device_grant_request`, `device_grant_poll` | device name, platform; `device_code` | `DeviceAuthorization{user_code, verification_uri, interval}`; `Token | Pending | SlowDown | Denied | Expired` | none |
| `exchange_mobile_code` | `code`, `code_verifier` (`POST /api/v1/auth/mobile-code-exchanges`) | `SessionTokens{access, refresh, expires_in}` | `InvalidCode` |
| `refresh` | none (reads the refresh token from `CredentialProvider`; `POST /api/v1/auth/refresh-tokens`) | `SessionTokens` | `Unauthorized` (revoked, expired or reuse detected: the host signs out) |
| `sign_out` | none | `()`: calls `DELETE /api/v1/devices/me` so the server revokes the calling credential; the host then clears its local copy | none |
| `cancel` | none | cancels every in-flight operation cooperatively (UniFFI has no async cancellation; the host calls this from `onStopped`) | none |

Common errors on every operation: `Transport`, `Cancelled`, `UpgradeRequired`, `Unauthorized`, `RateLimited{retry_after}`, `AccountNotActive`.

**Wire code mapping** (the only place wire codes become typed errors; unknown codes become `Unexpected{code, request_id}`):

| Wire | Typed | What the caller does |
|---|---|---|
| `StagedCapReached` | `StagedCapReached` | commit what is staged, then stage again (section 3, pacing) |
| `CommitIdReused` (same id, other request) | `CommitIdReused` | a bug or a lost checkpoint: drop the batch, build a new one with a new id |
| `BatchTooLarge` | `BatchTooLarge` | the executor already chunks by `ServerInfo.batch_max_entries`, so this is a bug report |
| `SizeTooLarge` | `SizeTooLarge` | permanent for this file: skip and report |
| `AccountNotActive` | `AccountNotActive` | stop all work, show "account disabled" |
| `BlocklistMismatch` (commit) | `BlocklistMismatch` | planner `RestartSession` after re-hashing (the file changed) |
| `ParentNotFound` / `ParentGone` | same names | re-list the parent; a vanished parent drops the pending entry |
| `NotAFolder` | `NotAFolder` | re-list; treat the target as changed |
| `quota_exceeded` (HTTP 507) | `QuotaExceeded` | pause with a visible message |
| `invalid_cursor` (HTTP 422) and 410 | `ResyncRequired` | head cursor first, then listing (section 3) |
| 409 `BlockConflict` | planner input `BlockConflict` | section 2 planner |
| 426 `upgrade_required` | `UpgradeRequired` | show the server text, no retry |

Host-implemented traits (callbacks): `CredentialProvider` with `bearer() -> Credential{access: Secret, refresh: Option<Secret>}` (called per request, never cached longer than a request) and `on_rotated(access, refresh)` (see token lifecycle in section 3; desktop device tokens carry no refresh token and never rotate), `FileSource` (`len`, `read_at`, `identity() -> (size, mtime_ns)`; the core re-checks identity before commit), `DownloadSink` (`write_verified(offset, bytes)`, `finish(verified)`; only verified blocks reach it), `CheckpointStore` (`load/save/clear(local_key) -> UploadCheckpoint{session_id, block_hashes, file_hash, commit_id, state}` plus the batch records in section 3; implemented by 14's LocalIndex and 17's checkpoint DB; `MemoryCheckpointStore` for tests), `ProgressSink` (`on_progress(done, total)`; must be cheap).

### Interface: UploadPlanner (sans-IO, also in WASM)
`new(block_count, need_blocks, lanes_max, retry_policy)`; `next_actions() -> [SendBlock{i} | Wait{ms} | RereadSession | Commit | Done | RestartSession]`; `on_result(i, Acked | HashRejected(422) | Transient{retry_after?} | BlockConflict{retry_after?} | SessionGone)`; `on_session(need_blocks)`; `on_commit(NeedBlocks{list} | BlocklistMismatch | Ok | SessionGone)`. Invariants: never more than `min(4, server lanes)` blocks in flight, never re-send an acked block unless the server re-lists it as needed, a 422 resends that block (bit flip, FT-N-01), `SessionGone` and `BlocklistMismatch` restart with the same `client_key` (after re-hashing for the second).

**409 `BlockConflict`** (03: the block is acked with other bytes, or another request for it is in flight): the planner answers `Wait{retry_after}` then `RereadSession`. If `need_blocks` still lists the block it was only in flight: resend (`Transient`, bounded by `sync.max_attempts_per_block`). If the block is no longer needed and this run sent that block with the same hash, it is `Acked`. Otherwise the stored bytes may differ from the local ones: `RestartSession`; the commit-time blocklist check is the final guard.

## 2a. Ports

Server components talk through `contracts` port traits. `contracts` is server-only (it pulls server dependencies), so it is not a client dependency: the client-side traits live in the client crate `sync-core`. Across a process or language boundary the schema named in "Defined in" applies. Format used in every client component: direction, port, defined in, counterpart.

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Provides | `SyncApi` (typed client API, section 2; implemented by `HttpSyncClient`, faked in tests) | `sync-core` | 14, 17 (`mobile_backup`) |
| Provides | block hashing, rollup, commit ID, `UploadPlanner` resume diff (WASM exports) | `sync-core-wasm` (wasm-bindgen API) | 16 |
| Requires | `CredentialProvider` (`bearer`, `on_rotated`) | `sync-core` | implemented by 14 (token store), 17 (`TokenVault` bridge) |
| Requires | `FileSource` | `sync-core` | implemented by 14 (`PlatformFs` handle), 17 (`AssetSource::open_read`) |
| Requires | `DownloadSink` | `sync-core` | implemented by 14; 17 does not use it in v1.0 |
| Requires | `CheckpointStore` (per-file checkpoints and batch records) | `sync-core` | implemented by 14 (LocalIndex), 17 (checkpoint DB); `MemoryCheckpointStore` for tests |
| Requires | `ProgressSink` | `sync-core` | implemented by 14, 17 |

Server calls are HTTP only (OpenAPI of 11); no server crate is imported.

## 3. Behaviour that matters
- **Protocol constants** come from `ServerInfo` fetched at connect: `block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, `longpoll_max_wait`, `min_client_version`, `api_versions`. A server block size other than 2 MiB is `ProtocolMismatch` (fixed by the protocol); lanes = `min(4, max_lanes)`; an `api_versions` list without the client's version or `min_client_version` above the client is `UpgradeRequired`; every number is range-checked and the server's limit wins over local settings.
- **Upload (large)**: hash (resumable per block) -> `start_upload(client_key, content_hash, base_revision)`; `NothingToSend{revision}` ends it with 0 bytes -> `PUT block i` with its BLAKE3 -> `commit`. Resume re-reads the session (`need_blocks`) so at most 4 x 2 MiB is in flight; with up to 8 MiB acknowledged but not yet checkpointed by the server, the worst-case re-send is 16 MiB (AC-3, G-03-1, AZ-3953). **Lost commit response** (network error or `UploadNotFound` at commit): call `start_upload` again with the same `client_key` and hash; `NothingToSend{revision}` means it was published. Memory: 4 x 2 MiB buffers plus hash state, no whole-file buffering (<= 150 MB on mobile, AC-52).
- **Small upload**: `stage_small` per file in parallel over HTTP/2, then one `batch_commit`. The host forms the batch (`commit_id` plus the member `staged_key`s) and **saves it with `CheckpointStore::save_batch(BatchRecord{commit_id, entries[{staged_key, parent, name, base_revision, file_hash}]})` before the first `stage_small` of any member**, so a kill at any point replays the same `commit_id` with the same entries: staging is idempotent by key and a repeated commit gets the stored response (AC-40, FT-N-38). A staged key the server no longer has is staged again from the source (identity re-checked). `load_batches` lists the unfinished ones at start and `clear_batch(commit_id)` runs after the host has stored the per-entry results. **Pacing**: staged-uncommitted files per account stay at or below half of `ServerInfo.max_staged_per_account` (other devices share the cap); `StagedCapReached` means: commit what is staged, then stage more. The executor also chunks batches by `batch_max_entries` and `sync.batch_max_bytes_in_flight`.
- **`client_key` and `commit_id`**: `client_key` names one logical upload of one content version and is the idempotency key of `start_upload`; it is also the `staged_key` of a small upload. Desktop and web: a random UUIDv7 created when the upload is first planned and stored in the checkpoint (desktop LocalIndex, web IndexedDB resume record), reused on every retry and resume, new for new content. Mobile: derived from `(device_id, asset_id, file_hash)` with helper 05 (`derive_client_key`, used as the per-asset key; stable across reinstall and across foreground and background runs). A batch `commit_id` is always a random UUIDv7 stored with the batch record; it is never derived from one asset. 13 only guarantees that the stored keys are reused on every retry.
- **Download**: ranged GET, rolling BLAKE3 per block against the revision blocklist (`get_revision`), whole-file rollup against `content_hash`; a mismatching block aborts and is never handed to the sink as good data (AC-7, FT-N-02); resume continues at the last verified block boundary.
- **Feed**: client timeout = `wait + longpoll_client_margin`; `has_more` re-polls immediately; errors back off with jitter and `Retry-After`. The host runs one feed loop and keeps one cursor per account (14 fans rows out to pairs). **Page checks (AC-69, client side)**: items strictly ascending by `(seq, id)`, each `seq` in `(cursor.seq, next.seq]`, `next.epoch == cursor.epoch`, `next >= cursor`; a violation is `ProtocolViolation`, never applied silently. The caller persists the cursor only after applying (F7). After `ResyncRequired` (410, or `invalid_cursor` 422) the caller reads `head_cursor` **first**, then `list_children` pages (F14 order), so nothing is missed.
- **Base revision tracking**: `upload` and `delete_node` of a file carry the caller's `base_revision`; `move_node` carries none. The server answers `ConflictCopy`/`EditedSince`, 13 maps them to typed outcomes and returns the new revision id for the host to store. A new-file name collision is not an error: the server assigns the conflict-copy name and answers `ConflictCopy{node, copy}`; the client never retries with a new name. 13 persists none of it (14: LocalIndex; 17: checkpoint DB).
- **Token lifecycle (mobile sessions)**: the access token lives 15 minutes and the refresh token rotates (01). Any 401 on a request that carried a refresh-capable credential triggers `refresh` **single-flight**: one refresh runs per `HttpSyncClient`, concurrent callers wait for its result and retry once. Order is fixed: the new refresh token is handed to `CredentialProvider::on_rotated(access, refresh)` and must be durably stored before the new access token is used for any request; the server's reuse grace (30 s) only covers a lost response, so a refresh that failed on transport is retried with the same refresh token. `Unauthorized` from `refresh` (revoked, expired, reuse detected) stops all work and is reported to the host. Long uploads and the feed loop survive a mid-transfer expiry: the 401 of a block PUT or a poll is refreshed and the call repeated, so no request holds a token longer than one call. If `on_rotated` fails to persist the new tokens, the client retries it once, then treats the refresh as rejected (`Unauthorized`, which 17 surfaces as `TokenRejected`) and the host signs out. Desktop device tokens have no refresh token and never rotate in v1.0: a 401 is `Unauthorized` at once (revocation, AC-21).
- **Version negotiation (AC-64)**: every request carries `X-Sunnycloud-Client: <family>/<semver>` and `Accept` for `/api/v1`; HTTP 426 (`upgrade_required`) becomes `UpgradeRequired` with the server text, surfaced to the UI instead of retrying.
- **Transport**: one reqwest 0.13.5 client per `HttpSyncClient`, rustls, TLS >= 1.3, ALPN h2, `https_only` (AC-54); the only way to allow `http://` is the compile-time feature `allow-plain-http` (debug builds; CI asserts release binaries lack it). Explicit windows (`http2_initial_stream_window_size`, `http2_initial_connection_window_size`, adaptive window off) sized above the bandwidth-delay product (1 Gbps x 20 ms = 2.5 MB; start 8 MiB stream / 32 MiB connection, tuned in gate S1/NFT-PERF-04), HTTP/2 keep-alive pings while idle so NATs keep a held long-poll. System trust store via rustls-platform-verifier; on Android the host calls `init_platform_verifier(env, context)` once (Kotlin component from the fact card).

## 4. Data owned, side effects
No persistent data (state is in host-implemented stores). In memory per operation: planner state, block buffers, HTTP connection pool. Network I/O only; reads host `FileSource`, writes host `DownloadSink`. Logs by structured events; **tokens never logged**.

## 5. Configuration (typed section `sync`, client settings reloadable per helper 01; durations never hard-coded)
`sync.server_url`, `sync.client_family`/`version` (build-injected), `sync.retry_base` (500ms), `sync.retry_max` (30s), `sync.max_attempts_per_block` (8), `sync.connect_timeout` (10s), `sync.request_idle_timeout` (60s), `sync.longpoll_wait` (30s, clamped to `ServerInfo`), `sync.longpoll_client_margin` (10s), `sync.h2_stream_window` / `sync.h2_conn_window` (8 MiB / 32 MiB), `sync.keepalive_interval` (20s), `sync.hash_cpu_share` (desktop 25% of a core while active, AC-15), `sync.hash_checkpoint_blocks` (32), `sync.batch_max_bytes_in_flight` (64 MiB). Server-provided limits win over local ones. Environment names follow helper 01: `SUNNYCLOUD_` + the key upper-cased with `.` replaced by `_` (`sync.retry_base` is `SUNNYCLOUD_SYNC_RETRY_BASE`).

## 6. Dependencies and graph
**Implement after**: helpers 01, 02, 05; OpenAPI for the used operations. **Parallel with**: 11, 14 (against a fake server), 17. **Blocks**: 14, 17, 18, 16 (WASM subset first). Key crates (pinned, `Cargo.lock` committed): blake3 1.8.7, reqwest 0.13.5 (rustls), tokio >= 1.54.1, h2 >= 0.4.16, bytes >= 1.11.1, rustls >= 0.23.45, rustls-platform-verifier 0.7.1, uniffi 0.32.2, wasm-bindgen =0.2.130 (adapter).

## 7. Security notes
HTTPS/TLS 1.3 only; credentials come from the host per request and are wrapped in `Secret`; URLs and error messages are scrubbed of tokens; no redirects are followed to another host; server-supplied names are returned as data, never used as paths (14/06 own path safety); all server-supplied numbers (limits, sizes, cursors) are range-checked; downloads never expose unverified bytes. Hash and commit-ID rules are shared byte for byte with the server through helper 05 and a vector file that gates CI.

## 8. ACs implemented
| AC | Release | Part owned |
|---|---|---|
| AC-2, AC-4 | v1.0 | client side of throughput and 50 GiB files (windows, lanes, constant-time commit) |
| AC-3 | v1.0 | resume re-sends <= 16 MiB (4 lanes x 2 MiB in flight + one 8 MiB server checkpoint interval; G-03-1) |
| AC-7 | v1.0 | block-by-block verified download, hashes on every upload |
| AC-8 | v1.0 | base revision on every mutation, typed conflict outcomes |
| AC-40 | v1.0 | single-request upload + batch commit by id |
| AC-41 | v1.0 | `ResyncRequired` handling primitives, head-first order |
| AC-46, AC-31 | v1.0 | `lookup_revisions` (reinstall rebuild) and listing `content_hash` (first-sync merge); `NothingToSend` for 0 bytes of known content |
| AC-52 | v1.0 Android, v1.1 iOS | one implementation for every mobile entry point, bounded memory, `cancel()` |
| AC-54 | v1.0 | refuses `http://` by default |
| AC-64 | v1.0 | version header, 426 -> `UpgradeRequired` |
**Interaction ACs folded in**: **AC-69** [v1.0] (client): feed-page invariant checks and idempotent, caller-persisted cursor; **AC-78** [v1.0 Android] is owned by 17 (`mobile_backup`); 13 contributes commit IDs reused on every retry and lost-commit recovery through `NothingToSend`, so a foreground and a background run yield one server file (the in-process run mutex and per-asset claim are 17's); AC-68 and AC-38 (client share): a block is `Acked` only on the server's 204, never earlier.

## 9. Tests
FT-P-01, FT-P-02, FT-P-03, FT-P-04, FT-P-05, FT-P-32, FT-P-34, FT-P-35, FT-N-01, FT-N-02, FT-N-26, FT-N-27, FT-N-38, NFT-PERF-04, NFT-PERF-05, NFT-PERF-07, NFT-PERF-22, NFT-PERF-23, NFT-RES-01, NFT-RES-02, NFT-RES-18, NFT-RES-22, NFT-RES-25, NFT-RES-27, NFT-RES-28, NFT-RES-33, NFT-RES-LIM-03, NFT-SEC-12, NFT-SEC-21. Unit/property: planner (random failure injection never exceeds 4 in flight, never loses a block; `BlockConflict` cases), page-invariant checker, `ServerUrl`, wire-code mapping table, single-flight refresh. Refresh fixture (NFT-RES-33): a fake server issues 15-minute access tokens (time-compressed through the clock) and rotating refresh tokens; four concurrent 401s cause one refresh, the new refresh token is stored before first use, a kill between server rotation and store is healed by the 30 s grace, and a 20-minute transfer completes. Integration against the real server stack (Compose test) and a fake server that drops connections, flips bits and delays responses. Conformance vectors shared with 16 (native = WASM = UniFFI output).

## 10. Component acceptance
A 5 GiB upload killed at random points resumes and re-sends <= 16 MiB; 10,000 small files commit in <= 60 s at 20 ms RTT with a retried batch causing no duplicates; a flipped bit in transit is rejected and re-sent; a download with one corrupt block fails and delivers nothing as valid; a server older/newer than the client yields a clear typed message; the desktop large transfer reaches >= 90% of 1 Gbps; native, WASM and Kotlin/Swift hashing produce identical file hashes on the shared corpus.

## 11. Logging
ERROR protocol violation, unexpected status; WARN retry with attempt and delay, resync required, lost-commit recovery; INFO connect with server version, upload finished (ids and byte counts); no per-block logging. Structured fields: `op`, `attempt`, `status`, `request_id` from the response; no names, paths or tokens.

## 12. Risks and open points
0. Default chosen at Step 5 gap pass: a failed `on_rotated` persist is retried once, then treated as `TokenRejected` and the app signs out.
1. A single HTTP/2 connection carries all four lanes: one TCP flow and head-of-line blocking after loss; if gate S1 misses 90% on lossy links, add a second connection (config) before anything else.
2. Lost-commit recovery: decided for every case in `contracts/13_sync_core/sync_api.md` § Lost-commit recovery (03 rule AZ-3732, client rule AZ-3949, committed-key answer by 03 per the owner's decision of 2026-11-13); a lost response never yields a second server file within `upload_session_ttl`.
3. Mobile cdylib assembly and checkpoint DB sit in 17; if 17 slips, the desktop path still works with `MemoryCheckpointStore`.
4. Android trust store needs the Kotlin verifier component and a Proguard rule; private-CA servers fail without it.
5. `ChangeItem` is the latest node state, not an event (data model D1); the caller derives create/update/move/delete, so 13 cannot offer an "op" field.
6. The UniFFI version and the `syn` pin must be handled in the workspace lockfile.
7. The WASM split stays as confirmed; trimming `sync-core-wasm` (size, exports) is a risk-register item.
