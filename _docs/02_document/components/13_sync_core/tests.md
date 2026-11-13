# Test Specification — 13 Sync Core

Component tests complement the system-level scenarios in `02_document/tests/` and reference them by ID. Tiers: **Unit** = `sync-core-proto` and `sync-core` with an in-process scripted fake HTTP server (workstation is fine); **IT** = `HttpSyncClient` against the real server container with real PostgreSQL (Compose `core` profile, env `C`); **C-REF** = timing valid only on REF-SRV/REF-LOAD, otherwise `NOT-RUN(HW-REF)`. Only description names are used (`SyncApi`, `UploadPlanner`, `CredentialProvider`, `CheckpointStore`, `FileSource`, `DownloadSink`, `ProgressSink`, `ServerInfo`). A fake `SyncApi` is used only by host-side unit tests (14, 17), never here.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-2 | Large transfer >= 90% of link speed | PT-13-1 | Covered; system NFT-PERF-04 (C-REF) |
| AC-3 | Interrupted upload re-sends <= 16 MiB (G-03-1) | IT-13-1, IT-13-2, AT-13-1 | Covered; system FT-P-01, NFT-RES-01 |
| AC-4 | Files up to 50 GiB, constant-time finish | IT-13-3, PT-13-3 | Covered; system FT-P-03, NFT-PERF-06 |
| AC-7 | Hash verification on every transfer | IT-13-4, IT-13-5, IT-13-20, AT-13-2 | Covered; system FT-N-01, FT-N-02 |
| AC-8 | Base revision on every mutation, typed conflicts | IT-13-6 | Covered; system FT-P-10, FT-P-12 |
| AC-21 | Revoked credential stops work | IT-13-9, IT-13-10, IT-13-19 | Covered; system NFT-SEC-08, NFT-RES-26 |
| AC-31 | Listing `content_hash` for first-sync merge | IT-13-14 | Covered; system FT-P-54 |
| AC-38 | Block acked only on the server's 204 | IT-13-15 | Covered; system NFT-RES-03 |
| AC-40 | Single-request upload, batch commit by id | IT-13-7, IT-13-8, PT-13-4 | Covered; system FT-N-38, NFT-PERF-07, NFT-RES-22 |
| AC-41 | `ResyncRequired`, head-first order | IT-13-12 | Covered; system FT-P-13, NFT-RES-07 |
| AC-46 | `lookup_revisions` rebuild, `NothingToSend` | IT-13-14, IT-13-16 | Covered (new-file leg unblocked 2026-11-11, AZ-3732); system FT-P-32 |
| AC-52 | One implementation for mobile; bounded memory; `cancel()` | IT-13-17, IT-13-18, PT-13-2 | Covered; system NFT-RES-LIM-03 |
| AC-54 | Refuses `http://` by default | IT-13-13, ST-13-1 | Covered; system FT-N-26, NFT-SEC-12 |
| AC-58 | Concurrent refreshes yield one successor (client) | IT-13-9 | Covered; system NFT-SEC-10, NFT-RES-33 |
| AC-64 | Version header, 426 to `UpgradeRequired` | IT-13-13, AT-13-2 | Covered; system FT-N-27 |
| AC-68 | No revision for a missing blob (client: ack on 204 only) | IT-13-15 | Covered (client share); server side owned by 02 |
| AC-69 | Feed page invariants, caller-persisted cursor | IT-13-11, IT-13-12 | Covered; system NFT-RES-35 |
| AC-70 | `move_node` has no base revision | IT-13-6 | Covered; system FT-N-42 |
| AC-76 | `InvalidCode` mapped on a reused mobile code | IT-13-9 | Covered (client mapping); server rule owned by 01 |
| AC-78 | Ids reused on every retry, lost-commit recovery | IT-13-16, IT-13-17 | Covered (13 contribution); system FT-P-76 |

## Blackbox Tests

### IT-13-1: Planner invariants under random failures (unit, property test)
- **Summary**: `UploadPlanner` never exceeds the lane limit and never loses a block.
- **Traces to**: AC-3, AC-52
- **Input**: seeded driver of `next_actions`/`on_result`/`on_session`/`on_commit`; 20 seeds x 1,000 runs; `block_count` in {1, 2, 25,600}; `lanes_max` in {1,2,4,8}; each result drawn from `Acked | HashRejected | Transient | BlockConflict | SessionGone` with failure probability 0, 0.1, 0.5.
- **Expected**: in flight <= `min(4, lanes_max)` at every step; an acked block is never re-sent unless `on_session` re-lists it; every needed block ends acked, or the run ends in `RestartSession`/`Done`; after `sync.max_attempts_per_block` (8) `Transient` results on one block the planner stops retrying it.
- **Max time / Deps**: 60 s; `sync-core-proto` only (no network, no clock).

### IT-13-2: Planner reactions to each wire result (unit, table)
- **Summary**: Each documented reaction yields the documented action.
- **Traces to**: AC-3, AC-7
- **Input**: (a) `on_session(need_blocks=[5,6,9])` of 100 blocks; (b) `HashRejected` on block i; (c) `BlockConflict{retry_after=2s}` with i still needed; (d) `BlockConflict`, i not needed, this run sent i with the same hash; (e) same but not sent by this run; (f) `on_commit(NeedBlocks{[7]})`, `on_commit(BlocklistMismatch)`; (g) `SessionGone`.
- **Expected**: (a) only blocks 5, 6, 9 sent; (b) i sent again; (c) `Wait{2000}` then `RereadSession`, then resend; (d) treated `Acked`; (e) `RestartSession`; (f) `SendBlock{7}` then `Commit`; mismatch -> `RestartSession` after re-hash with the same `client_key`; (g) `RestartSession`, same `client_key`. `Commit` is never emitted while a needed block is outstanding.
- **Max time / Deps**: 2 s; `sync-core-proto`.

### IT-13-3: Large upload against the real server
- **Summary**: A large upload finishes in constant time and the stored hash equals the client hash.
- **Traces to**: AC-4
- **Input**: DS-09 (5 GiB) in `C`; DS-10 (50 GiB sparse-hybrid) in `C-LARGE`; `shape-wan`.
- **Expected**: `CommitOutcome::Published`; server file hash equals local BLAKE3; last block ack to `Published` <= 2 s for both sizes.
- **Max time / Deps**: 20 min (5 GiB), 4 h (50 GiB); server, `blobs-large` for 50 GiB.

### IT-13-4: Verified download rejects a corrupt block
- **Summary**: One flipped block gives `HashMismatch{block}` and the sink never sees it.
- **Traces to**: AC-7
- **Input**: fake server, 40 MiB revision with one bit flipped in block 7; `DownloadSink` spy.
- **Expected**: `Err(HashMismatch{block: 7})`; sink offsets cover blocks 0 to 6 only; `finish(verified=true)` never called; a rerun with `resume_from = 7 * 2 MiB` against a clean server returns `Verified{file_hash}`.
- **Max time / Deps**: 10 s; fake server then real server.

### IT-13-5: Download resumes at the last verified boundary
- **Summary**: After a reset at most one unverified block is re-read.
- **Traces to**: AC-7
- **Input**: DS-09, TCP reset at 50% (`shape-flaky`).
- **Expected**: bytes requested after the reset <= 2.5 GiB + 2 MiB; `Verified{file_hash == content_hash}`; each block reaches the sink once.
- **Max time / Deps**: 10 min; server, `e2e-ctl` network drop.

### IT-13-6: Base revision and conflict outcomes
- **Summary**: `upload`, `delete_node`, `move_node` carry the base revision as specified.
- **Traces to**: AC-8, AC-70
- **Input**: two clients, one account. A uploads `f.txt` (rev1); B uploads rev2; A calls `upload` with `base_revision = rev1`, then `delete_node(f.txt, rev1)`, then `move_node` of the parent; request bodies from the proxy log; also a new-file name collision.
- **Expected**: `ConflictCopy{node, copy}` with a server-chosen name; `Err(EditedSince{current: rev2})`; the move request has no `base_revision` and keeps the node id; the collision yields `ConflictCopy` with exactly 1 request for the name (no client retry).
- **Max time / Deps**: 15 s; server.

### IT-13-7: Batch commit, save-before-stage, replay
- **Summary**: The batch record is saved before the first `stage_small`; kills replay the same `commit_id`.
- **Traces to**: AC-40
- **Input**: `CheckpointStore` spy over `MemoryCheckpointStore`; 300 files (DS-23 sizes 2 MiB - 1, 2 MiB, 2 MiB + 1 included); kills after `save_batch`, after 150 stagings, after `batch_commit` was sent.
- **Expected**: `save_batch` precedes every `stage_small` of its batch; restart `load_batches` returns the record and the same `commit_id` and entries are replayed; the server returns the stored response; 300 server files, 0 duplicates; `clear_batch` runs after results are stored; the 2 MiB + 1 file takes the block path.
- **Max time / Deps**: 60 s; server; system FT-N-38, NFT-RES-22.

### IT-13-8: Batch chunking and staged-cap pacing
- **Summary**: Chunks respect `batch_max_entries` and `sync.batch_max_bytes_in_flight`; `StagedCapReached` commits first.
- **Traces to**: AC-40
- **Input**: fake `ServerInfo{batch_max_entries: 100, max_staged_per_account: 200}`; 1,000 small files; fake answers `StagedCapReached` at 200 uncommitted.
- **Expected**: no batch above 100 entries; staged-uncommitted seen by the fake <= 100 (half of the cap); on `StagedCapReached` the executor commits, then stages more; 0 `BatchTooLarge`.
- **Max time / Deps**: 30 s; fake server.

### IT-13-9: Token refresh: single-flight, persist-before-use, failures
- **Summary**: Concurrent 401s cause one refresh; the new refresh token is stored before the new access token is used.
- **Traces to**: AC-58, AC-21, AC-76
- **Input**: fake server with 15-minute access tokens (test clock) and rotating refresh tokens (NFT-RES-33 fixture); `CredentialProvider` spy whose `on_rotated` sleeps 300 ms.
- **Expected**: (a) 4 concurrent 401s -> exactly 1 refresh, all 4 retry once and succeed; (b) no request carries the new access token before `on_rotated` returned, which is called once with `(access, refresh)`; (c) refresh fails on transport -> retried with the same refresh token, accepted inside the 30 s grace; (d) refresh 401/revoked -> `Unauthorized`, in-flight ops end, no further requests; (e) reused mobile code -> `InvalidCode`. BLOCKED sub-case: behaviour when `on_rotated` itself fails is not specified (no error type in the trait).
- **Max time / Deps**: 30 s; fake server, `ManualClock`.

### IT-13-10: Desktop device token never refreshes
- **Summary**: A credential without a refresh token makes a 401 an immediate `Unauthorized`.
- **Traces to**: AC-21
- **Input**: `Credential{access, refresh: None}`; next poll answers 401.
- **Expected**: `Unauthorized` within 1 s; refresh endpoint hits 0; `on_rotated` never called.
- **Max time / Deps**: 2 s; fake server.

### IT-13-11: Feed page invariant checker
- **Summary**: Each violated invariant is a `ProtocolViolation`, never applied.
- **Traces to**: AC-69
- **Input**: pages around cursor `1.100`: v1 not ascending by `(seq, id)`; v2 item `seq <= 100`; v3 item `seq > next.seq`; v4 `next.epoch != cursor.epoch`; v5 `next < cursor`; v6 duplicate `(seq, id)`; valid page seq 101..150, `next` 1.150, `has_more=true`.
- **Expected**: v1 to v6 -> `Err(ProtocolViolation{detail})` naming the rule; valid page returned and `has_more` re-polls with no wait.
- **Max time / Deps**: 2 s; fake server.

### IT-13-12: Resync order
- **Summary**: 410 and 422 `invalid_cursor` become `ResyncRequired`; the host-side order is head first, listing second.
- **Traces to**: AC-41, AC-69
- **Input**: real server restored from an older `pg_dump` (`e2e-ctl`) while a client polls `1.500`; a crafted 422.
- **Expected**: `ResyncRequired{reason}` and `ResyncRequired{InvalidCursor}`; the harness helper calls `head_cursor` before the first `list_children` (request log); `CheckpointStore` has no cursor method, so 13 never persists the cursor.
- **Max time / Deps**: 60 s; server, `e2e-ctl` restore (group `X`).

### IT-13-13: Connect, version negotiation, wire-code table, plain HTTP
- **Summary**: `connect` validates `ServerInfo`; every wire code maps to its typed error.
- **Traces to**: AC-64, AC-54
- **Input**: fake server cases: `block_size` 1 MiB; `min_client_version` above the client; `api_versions` without v1; `max_lanes` 2 and 16; `http://` URL; HTTP 426; every row of the wire table; unknown code `weird_code` with request id `r-1`.
- **Expected**: `ProtocolMismatch`; `UpgradeRequired{message, update_url}`; `UpgradeRequired`; lanes 2 and 4; `InsecureScheme` with 0 sockets opened; 426 -> `UpgradeRequired` with 0 retries; each row maps as documented; unknown -> `Unexpected{code: "weird_code", request_id: "r-1"}`; every request except `GET /api/v1/server-info` carries `X-Sunnycloud-Client`.
- **Max time / Deps**: 10 s; fake server.

### IT-13-14: Listings and `lookup_revisions`
- **Summary**: Listing items carry `content_hash`; lookups are chunked at 1000.
- **Traces to**: AC-31, AC-46
- **Input**: 2,500 files (DS-01 subset) listed in pages; `lookup_revisions` of 2,500 known and 10 unknown hashes.
- **Expected**: every file item has the right BLAKE3, folders none, pages chain to `next=None`; 3 lookup calls (1000, 1000, 510); 2,500 `RevisionRef` with `client_meta`.
- **Max time / Deps**: 20 s; server.

### IT-13-15: Ack only on 204
- **Summary**: A block is done only on the server's success status.
- **Traces to**: AC-38, AC-68
- **Input**: fake answers block PUT with 200 plus body, 202, 500 after reading the body, then a reset after the full body.
- **Expected**: each non-204 outcome leaves the block in `need_blocks` and it is re-sent; `ProgressSink::on_progress` counts acked blocks only.
- **Max time / Deps**: 10 s; fake server; kill variant NFT-RES-03.

### IT-13-16: Lost commit response
- **Summary**: A commit whose answer is lost is recovered by the same `client_key`.
- **Traces to**: AC-78, AC-46
- **Input**: the proxy drops the commit response once (`e2e-ctl`). Case A: new revision of an existing node (base revision set). Case B: brand-new file at `(parent, name, hash)`.
- **Expected**: A: `start_upload` with the same `client_key` -> `NothingToSend{revision}`, 1 revision, 0 block bytes re-sent. B: a NEW file whose commit response was lost: `start_upload` with the same parent, name, `client_key` and file hash -> `NothingToSend{revision}` of the live file at that name (03 rule decided 2026-11-11, AZ-3732, `contracts/03_transfer/upload_service.md`), 1 revision, no conflict copy. C: a new file whose name is taken at commit by another device's file (or changed before the retry): `start_upload` with the same `client_key` -> `NothingToSend{revision}` of the committed revision (03 keeps the committed key until `upload_session_ttl`, owner 2026-11-13), 1 server file. Client rule: `contracts/13_sync_core/sync_api.md` § Lost-commit recovery (AZ-3949).
- **Max time / Deps**: 30 s; server, `e2e-ctl`.

### IT-13-17: `client_key` and `commit_id` rules
- **Summary**: Stored keys are reused; mobile keys derive from `(device_id, asset_id, file_hash)`.
- **Traces to**: AC-52, AC-78
- **Input**: desktop upload retried 3 times (`MemoryCheckpointStore`); mobile key for `(dev-1, asset-42, H)` in two processes and for `H'`; 5 batches.
- **Expected**: one UUIDv7 across the 3 attempts, new for new content; equal for the same triple, different for `H'`; every batch `commit_id` distinct random UUIDv7, never equal to an asset `client_key`.
- **Max time / Deps**: 2 s; helper 05 `derive_client_key`.

### IT-13-18: `cancel()` and bounded memory
- **Summary**: `cancel` stops every in-flight op; no op buffers a whole file.
- **Traces to**: AC-52
- **Input**: 3 concurrent 2 GiB uploads, `cancel()` at 5 s; RSS sampled every 1 s during a 5 GiB upload.
- **Expected**: all return `Cancelled` within 1 s and send nothing afterwards; RSS <= 150 MB.
- **Max time / Deps**: 5 min; server.

### IT-13-19: Rate limit, account state, hostile names
- **Summary**: `RateLimited{retry_after}` is honoured, `AccountNotActive` stops work, names stay data.
- **Traces to**: AC-21
- **Input**: 429 with `Retry-After: 3`; 403 `AccountNotActive`; listing names `../../etc/passwd`, `a‮b`, an 8 KiB name.
- **Expected**: next attempt not before 3 s (+ jitter < 1 s); no requests after `AccountNotActive`; names returned verbatim, never used as paths.
- **Max time / Deps**: 10 s; fake server.

### IT-13-20: Cross-implementation conformance vectors
- **Summary**: Native, WASM and UniFFI outputs agree byte for byte.
- **Traces to**: AC-7, AC-52
- **Input**: shared vector file over sizes 0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB, 100 MiB.
- **Expected**: identical block hashes, rollups and commit ids from `sync-core-proto`, `sync-core-wasm` (node and Chromium) and the Kotlin UniFFI build; 0 differences.
- **Max time / Deps**: 5 min; WASM build; emulator (EMU) for the UniFFI leg.

## Performance Tests

### PT-13-1: Client large transfer at 1 Gbps (gate S1, client side)
- **Summary**: `HttpSyncClient` reaches at least 90% of calibrated goodput.
- **Traces to**: AC-2
- **Load**: 1 client, 4 lanes, 5 GiB (DS-09) up and down, encryption at rest on; `iperf3` calibration first; windows 8 MiB stream / 32 MiB connection.
- **Targets / failure**: throughput >= 112 MB/s / below 100 MB/s or 90% of calibrated; error rate 0% / any failed block; client CPU below one full core / client is the bottleneck. RSS <= 150 MB. Env C-REF; system NFT-PERF-04.

### PT-13-2: Mobile-profile memory
- **Summary**: Memory is bounded for a 5 GiB file and 50,000 small files.
- **Traces to**: AC-52
- **Load**: mobile configuration, DS-09 plus DS-21 files; RSS every 1 s.
- **Targets / failure**: RSS peak <= 100 MB / above 150 MB; buffers <= 4 x 2 MiB / any whole-file buffer. System NFT-RES-LIM-03.

### PT-13-3: Planner cost at 50 GiB
- **Summary**: Planning 25,600 blocks is cheap and constant-memory.
- **Traces to**: AC-4
- **Load**: `UploadPlanner::new(25_600, all, 4, policy)`, 25,600 `Acked` results. Targets are proposed by this spec; the description gives none.
- **Targets / failure**: `next_actions` mean <= 50 us / above 1 ms; planner state <= 4 MB / above 16 MB.

### PT-13-4: 10,000 small files in 60 s
- **Summary**: Staging plus batch commit meets the AC at 20 ms RTT.
- **Traces to**: AC-40
- **Load**: DS-01, `shape-wan`, 1 client, parallel `stage_small`, batches of `batch_max_entries`.
- **Targets / failure**: total <= 45 s / above 60 s; errors 0 / any entry error; duplicates after a retried batch 0 / any. RSS <= 150 MB. Env C-REF; system NFT-PERF-07.

## Security Tests

### ST-13-1: Transport policy
- **Summary**: Only TLS 1.3 with ALPN h2; `http://` refused.
- **Traces to**: AC-54
- **Attack vector**: downgrade to plaintext, TLS 1.2, HTTP/1.1.
- **Procedure**: (1) `connect("http://sunnycloud.test")` in a release-profile build; (2) connect to a TLS 1.2-only and an h2-less fake; (3) check release binaries for the `allow-plain-http` feature.
- **Pass**: `InsecureScheme` and 0 packets captured (tcpdump); handshake failure for 2; feature absent in release (CI check). **Fail**: bytes to port 80 or the feature in a release binary.

### ST-13-2: Tokens never leak
- **Summary**: Tokens are absent from logs, errors and URLs.
- **Traces to**: AC-54, AC-58
- **Attack vector**: log and error inspection after all failure paths of IT-13-9 and IT-13-13.
- **Procedure**: run with sentinel tokens `ACC-SENTINEL`, `REF-SENTINEL`; collect DEBUG logs, `Display` and `Debug` of all errors, and all request URLs.
- **Pass**: 0 occurrences (`Secret` redacts). **Fail**: 1 or more.

### ST-13-3: No cross-host redirects; range-checked server numbers
- **Summary**: A malicious server cannot divert credentials or force huge allocations.
- **Traces to**: AC-54
- **Attack vector**: 302 to `https://evil.test/`; `ServerInfo` with `small_file_max = 2^63`, `batch_max_entries = 0`, `max_lanes = 0`, negative `longpoll_max_wait`; `next_cursor.seq = u64::MAX`.
- **Procedure**: serve each case; watch requests at `evil.test`; sample allocation.
- **Pass**: redirect not followed (0 `Authorization` headers at `evil.test`); each case ends in a typed error or a documented clamp within 2 s; allocation <= 64 MiB. **Fail**: panic, hang or a credential sent elsewhere.

## Acceptance Tests

### AT-13-1: Killed 5 GiB upload resumes
- **Summary**: Random kills cost at most 16 MiB each (AC-3; G-03-1, AZ-3953).
- **Traces to**: AC-3
- **Preconditions**: server, DS-09, `shape-flaky`.
- **Steps**: (1) upload 5 GiB and drop the connection at 10 seeded random offsets; (2) sum block PUT bytes per resume from the proxy log: each <= 16 MiB, no re-hash from zero (checkpoint); (3) server hash equals local BLAKE3. System FT-P-01, NFT-RES-01.

### AT-13-2: Flipped bit, corrupt download, wrong server version
- **Summary**: Corruption is rejected and an unsupported server gives a clear message.
- **Traces to**: AC-7, AC-64
- **Preconditions**: fake and real server.
- **Steps**: (1) flip one bit of an upload block in transit: 422, block re-sent once, file commits with the right hash; (2) corrupt one block of a download: `HashMismatch`, nothing delivered as valid; (3) connect to a server whose `min_client_version` is above the client: `UpgradeRequired` with the server text, no retry. System FT-N-01, FT-N-02, FT-N-27.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-09 / DS-10 | 5 GiB dense; 50 GiB sparse-hybrid | `gen_bigfile.py` | 5 GiB / 800 MB allocated |
| DS-01, DS-23 | 10,000 small files; sizes around 2 MiB | `gen_tree.py` | 1 GB |
| DS-21 | phone fixtures (key derivation, memory) | `gen_phone.py` | 1 GB |
| Vector file | hash and commit-id vectors shared with 16, 17 | repository fixture | < 1 MB |

**Setup**: unit and fake-server tests need no stack (in-process axum fixture with scripted drop, bit flip, delay, 401, 410, 422, 426, 429). IT tests: `tests/run.sh --profile core`, account via invite token, golden snapshot; fault tests start from a reset stack (group `X`).
**Teardown**: purge the account through `e2e-ctl` (not the deletion API); delete temp files and checkpoint DBs; logs stay in `e2e-results/`, never committed.
**Isolation**: one account and folder prefix per test; `MemoryCheckpointStore` per test; integration tests always use the real server container with real PostgreSQL.
