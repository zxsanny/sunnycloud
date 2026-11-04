# Test Specification — 03_transfer

Tier: component integration (`cargo test -p transfer`) at the public port boundary: `UploadService`, `CommitService`, `DownloadService`, `AccountLifecycleHook`, `ReservationSource`, `Janitor` (`SessionReaper`, `ReceiptJanitor`, `ProgressTable::flush_idle`) and the `CommitEffect` impls (`BlobCommitEffect`, `ReceiptEffect`). Env class `C`; timing rows `C-REF`. System scenarios are referenced by ID, not restated.
Collaborators are real: 02 `TreeCommit`/`TreeReader`, 04 `BlobWriter`/`BlobReader`/`BlobRegistry`/`BlobCatalog`/`GcQueue` on a temp directory, 07a `QuotaLedger`/`QuotaAdmin`, 05 stamp, all on one PostgreSQL 18. Fault injection uses thin decorators that delegate to the real port (fail or delay `await_durable`, fail `write_complete` after the write, record calls); nothing replaces storage. Clock: manual test clock; session TTL, reap interval, checkpoint interval are millisecond config values.
## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|------------------------------|----------|----------|
| AC-2 | Large transfer >= 90% of link | PT-03-1, PT-03-4 | Covered here (server ingest/egress) + NFT-PERF-02, NFT-PERF-03 |
| AC-3 | Resume re-sends <= 10 MiB | IT-03-4, AT-03-1 | Covered here + FT-P-01, NFT-PERF-05, NFT-RES-01 (see gap G-03-1: worst case can exceed 10 MiB) |
| AC-4 | 50 GiB; constant-time finish | IT-03-6, PT-03-2, PT-03-5 | Covered here (1 GiB / 8 GiB scale) + FT-P-03, NFT-PERF-06 (50 GiB) |
| AC-7 | Per-block BLAKE3, whole-file hash from blocks | IT-03-2, IT-03-6, ST-03-2 | Covered here + FT-N-01, FT-N-02, NFT-SEC-21 |
| AC-8 | Conflict copy surfaced on commit | IT-03-7 | Covered here + FT-P-10 |
| AC-27 | Account erase of sessions, staged rows, receipts | IT-03-15 | Covered here + FT-P-19, NFT-RES-30 |
| AC-30 | Erase queues blobs `Due::Now` | IT-03-15 | Covered here + NFT-SEC-27 |
| AC-38 | Ack only after durable | IT-03-3 | Covered here + FT-P-06, NFT-RES-03 |
| AC-39 | Session cap, quota, free-space floor | IT-03-1, ST-03-5 | Covered here + NFT-RES-05, NFT-RES-LIM-01 |
| AC-40 | Staged upload + batch commit with receipts | IT-03-5, IT-03-8, IT-03-10, PT-03-3 | Covered here + FT-P-04, FT-N-38, NFT-PERF-07, NFT-RES-22 |
| AC-42 | Name rules at start | IT-03-1 | Covered here + FT-N-04 |
| AC-43 | Range read 4 KiB p95 <= 5 ms (read side) | IT-03-13, PT-03-4 | Covered here + NFT-PERF-11, FT-P-42 |
| AC-47 | Inline allowlist decision | IT-03-13, ST-03-4 | Covered here + FT-N-11, FT-P-39, NFT-SEC-19 |
| AC-50 | `client_key` resume, restart on changed key | IT-03-1 | Covered here + FT-P-67, FT-N-03 |
| AC-51 | Range and bounded memory on download | IT-03-13, PT-03-5 | Covered here + FT-P-68 |
| AC-59 | Two-account matrix | ST-03-1 | Covered here + NFT-SEC-13 |
| AC-68 | Orphan blob, reserved quota released at expiry | IT-03-9, IT-03-10, IT-03-17 | Covered here + NFT-RES-34 |
| AC-69 | Effects add nothing after the stamp | IT-03-19 | Covered here + NFT-RES-35 |
| AC-70 | Commit names node by id; `parent_id` hint only | IT-03-18 | Covered here + FT-N-42 |
| AC-74 | Reserve/release exactly once | IT-03-9, IT-03-10, IT-03-12 | Covered here + NFT-RES-38 |
| AC-77 | Download side follows current revision | IT-03-13 | Covered here + FT-N-44 |

## Blackbox Tests

### IT-03-1: start_upload decisions
**Summary**: every `start_upload` outcome and error, including resume and the name/parent checks.
**Traces to**: AC-39, AC-42, AC-50
**Description**: quota 10 GiB, `max_open_sessions = 8`, `max_declared_size = 50 GiB`, floor probe reading a configurable value.
**Input data**:
```
valid: declared 10 MiB -> Started{need_blocks = 0..4, block_size = 2 MiB, lane_limit = 4, expires_at}
repeat same (account, client_key) -> same upload_id; repeat with other declared_size -> ClientKeyMismatch; different key -> new session
51 GiB -> SizeTooLarge; 9th open session -> SessionCapReached (reservation rolled back); declared 11 GiB -> QuotaExceeded
free space below floor -> FreeSpaceFloor; name "a/b" -> InvalidName; missing parent -> ParentNotFound; file parent -> NotAFolder
existing file with content_hash h and start with content_hash h -> NothingToSend{revision} (0 bytes)
begin_deletion account -> AccountNotActive; file creation failure injected -> compensating tx leaves 0 session rows, 0 blob rows, reserved_bytes unchanged
```
**Expected result**: as listed; every refusal leaves `QuotaAdmin::view.reserved_bytes` unchanged; the session cap check is serialised by the quota row lock (16 concurrent starts yield exactly 8 sessions).
**Max execution time**: 15 s; **Dependencies**: PostgreSQL, 02, 04, 07a

### IT-03-2: put_block verification and caps
**Summary**: hash/length are checked before any write; duplicates, conflicts and caps map to typed errors.
**Traces to**: AC-7, AC-38
**Input data**: 10 MiB file = 5 blocks (last 2 MiB exact? use 9 MiB: 4 full + 1 MiB tail); cases: correct; one flipped bit with the original `expected_hash`; body 1 byte short/long; chunked without length; index 5 and -1 (out of range); same block twice with same hash; same index with other bytes after ack; 5th concurrent lane; account in-flight cap 32 reached; `max_buffered_block_bytes` exhausted.
**Expected result**:
```
correct: BlockAck after durable; flipped bit -> HashMismatch with file bytes at that offset unchanged (still preallocated zeros) and bit unset; short/long/chunked -> LengthMismatch; index -> IndexOutOfRange
duplicate same hash -> BlockAck (idempotent, no second write); other bytes after ack -> BlockConflict, file unchanged; lane/account/global exceed -> Overloaded and 0 body bytes consumed (stream untouched)
tail block length = remaining bytes (1 MiB), not 2 MiB
```
**Max execution time**: 15 s; **Dependencies**: PostgreSQL, 04

### IT-03-3: Ack only after the group fdatasync
**Summary**: `put_block` returns `BlockAck` only after `await_durable`; an `Io` error means no ack and no bit.
**Traces to**: AC-38
**Description**: decorator delays `await_durable` by 300 ms, then a run where it returns `Io`.
**Expected result**: `put_block` unresolved for >= 300 ms and bit unset until the decorator releases; with `Io` -> `Storage` error, bit unset, `get_upload.need_blocks` still lists the index, next `put_block` of the same bytes succeeds.
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, 04

### IT-03-4: Resume after drop and after restart
**Summary**: re-sent bytes after a connection drop and after a service restart are bounded.
**Traces to**: AC-3
**Description**: 100 MiB upload, 4 lanes. Case A: abort 4 in-flight bodies mid-stream (connection drop). Case B: after 30 blocks acked drop the service instance (new `ProgressTable`), `checkpoint_bytes = 8 MiB`, `checkpoint_interval = 1 s`, kill right after the last ack and before the checkpoint (worst case).
**Expected result**:
```
A: need_blocks = only the in-flight blocks, re-sent <= 4 x 2 MiB = 8 MiB, ProgressTable intact
B: get_upload (lazy load from DB checkpoint) lists acked-but-uncheckpointed blocks again; measured re-send recorded; assertion re-send <= 10 MiB
```
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, 04. Note: with 4 lanes in flight (8 MiB) plus up to 8 MiB uncheckpointed the worst case is 16 MiB; B may legitimately fail the 10 MiB assertion (gap G-03-1).

### IT-03-5: stage_small
**Summary**: single-request uploads, zero bytes, boundaries and idempotence.
**Traces to**: AC-40
**Input data**: lengths 0, 1, 2 MiB-1, 2 MiB, 2 MiB+1; repeat same key/hash/size; same key other hash; wrong `expected_hash`; `max_staged = 1000` reached; quota exhausted; floor below.
**Expected result**:
```
0 bytes: Staged{size 0}, upload_sessions row block_count = 0, declared_size 0, empty blob; 1..2 MiB: block_count = 1
2 MiB+1 -> typed error (LengthMismatch or size error; no row, no file); repeat -> Staged, no write; other hash -> ClientKeyMismatch
wrong hash -> HashMismatch, no file, no row; StagedCapReached / QuotaExceeded / FreeSpaceFloor refused BEFORE any byte is written (blob directory file count unchanged)
failure injected after write_complete (reserve refused) -> compensating discard: file gone, 0 rows, reserved unchanged
```
**Max execution time**: 12 s; **Dependencies**: PostgreSQL, 04, 07a

### IT-03-6: commit_upload, hash roll-up and sniffing
**Summary**: commit validates the blocklist, derives the file hash from block hashes and sniffs the type from bytes only.
**Traces to**: AC-4, AC-7
**Input data**: JPEG bytes uploaded under name `photo.png` with client type `text/html`; HTML bytes under `x.jpg`; random bytes (unknown); an upload with block 2 missing; blocklist with one wrong hash; blocklist of 3 hashes for 5 blocks.
**Expected result**:
```
Committed{node_id, revision_id}; revision.content_hash == BLAKE3(file) computed independently (matches the 32-byte hash roll-up); BlobReader reads during commit: read_head only (decorator counts: 1 call, <= 64 KiB, 0 full reads)
media_type: image/jpeg; text/html; NULL for unknown (served as attachment)
missing block -> NeedBlocks([2]); wrong hash or count -> BlocklistMismatch; unknown/reaped id -> UploadNotFound; session row deleted on success
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, 02, 04

### IT-03-7: Commit target choice and conflict copy
**Summary**: `base_revision` selects `Target::Existing`; none selects `Target::New`; a taken name is not an error.
**Traces to**: AC-8
**Expected result**: with base (current) -> same node id, new revision; with stale base -> `Committed{conflict_copy}` and the old content in the copy (no `EditedSince`: see gap G-03-3); without base and name taken -> `Committed{conflict_copy}` named `name (conflict copy ... device)`, `device` equals `CommitActor.device_label`; no `name_conflict` code produced.
**Max execution time**: 8 s; **Dependencies**: PostgreSQL, 02

### IT-03-8: commit_batch and receipts
**Summary**: receipts make batches idempotent and atomic; entries fail independently.
**Traces to**: AC-40
**Description**: stage 500 files, batch with `commit_id = C1`; entries 7 (bad name) and 300 (parent tombstoned) invalid.
**Input data**: same batch re-sent; same `commit_id` with a changed entry; 501 entries; two identical batches from 2 connections at once; failed entries re-sent under a new id.
**Expected result**:
```
First: 498 Committed, 2 EntryError, one seq; receipt row stores all 500 outcomes (failed ones too)
Re-send identical: stored response returned (Replayed), 0 new nodes/revisions, no quota change; changed request -> CommitIdReused; 501 -> BatchTooLarge
Concurrent identical: exactly one applies, the other gets the stored response; no unique-violation error anywhere
New id for the 2 failed entries: they commit once fixed
Another account using C1: independent (no cross-account replay)
```
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, 02, 04, 07a

### IT-03-9: Effects inside the commit transaction; reaper vs commit
**Summary**: `BlobCommitEffect` locks the session row; a reaped session fails only that entry and rolls back its quota release; reaper and commit never both release.
**Traces to**: AC-68, AC-74
**Description**: (a) reap the session between `commit_upload` validation and `publish` (decorator pause), then publish. (b) 500 iterations: `SessionReaper::run_cycle` racing `commit_upload` on one expired-or-expiring session.
**Expected result**:
```
(a) UploadNotFound; no revision; usage unchanged; reserved_bytes released exactly once (by the reaper)
(b) per iteration exactly one winner: commit won -> used = size, reserved 0, blob committed; reaper won -> used 0, reserved 0, blob row and file removed; never reserved < 0, never used + reserved > expected
```
**Max execution time**: 60 s; **Dependencies**: PostgreSQL, 02, 04, 07a

### IT-03-10: Reaper, abort and checkpoints
**Summary**: expiry releases exactly the row's `reserved_bytes` once; rolling TTL; staged rows expire too.
**Traces to**: AC-68, AC-74
**Input data**: `upload_session_ttl = 400 ms`; 20 open sessions (10 touched by a block at +300 ms), 20 staged rows; `abort_upload` on one; `ProgressTable::flush_idle` with idle > checkpoint interval; 100 receipts aged 0..2 x `commit_receipt_ttl` for `ReceiptJanitor`.
**Expected result**: after TTL the 10 untouched sessions and all untouched staged rows are deleted, quota `reserved_bytes` falls by exactly their sum, files discarded only after commit of the delete tx; touched sessions survive (rolling TTL); `abort_upload` = same effect for one row; `flush_idle` writes `received`/`block_hashes` checkpoints and evicts; running the cycle twice releases nothing more; only receipts older than the ttl are deleted and a replay of a purged id is treated as new.
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, 04, 07a

### IT-03-12: ReservationSource equals open reservations
**Summary**: `open_reserved_bytes` equals the sum over open sessions and staged rows.
**Traces to**: AC-74
**Expected result**: after random starts, aborts, commits and expiries `open_reserved_bytes(account) == QuotaAdmin::view.reserved_bytes`; `QuotaAdmin::recompute(account, live_bytes, reserved_bytes)` from the two sources is a no-op (no drift).
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, 02 `LiveUsageSource`, 07a

### IT-03-13: Download semantics
**Summary**: ranges, validators, scoping, retained revisions and the inline/attachment decision.
**Traces to**: AC-43, AC-47, AC-51, AC-77
**Input data**: 5 MiB file; `Range: bytes=0-0`, `bytes=4194304-`, `bytes=-4096`, `bytes=0-1,5-9` (multi), overlapping multi, `bytes=9999999-`; `If-Range` current/stale ETag; `If-None-Match` equal; non-current revision as user and with `retained_ok`; folder; tombstoned; foreign id; sniffed types jpeg/png/gif/webp/avif/mp4/webm vs html/svg/xml/js/pdf/unknown.
**Expected result**:
```
Full 200 (total_size, ETag = blob id), Partial 206 with exact bytes (compared with source), multi/overlap ignored -> Full, unsatisfiable -> RangeNotSatisfiable, If-Range stale -> Full, If-None-Match -> NotModified
non-current revision: NotFound for user, served when retained_ok; folder/tombstoned/foreign -> NotFound; node resolved by NodeRef(current) follows a new current revision
disposition Inline exactly for the 7 allowlisted sniffed types, Attachment otherwise (file name never consulted)
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, 02, 04

### IT-03-14: Corruption and missing blobs on read
**Summary**: a failed tag ends the stream with an error; a missing blob is `ContentUnavailable`.
**Traces to**: AC-7, AC-43
**Description**: flip one byte in chunk 40 of a 10 MiB blob (outside the service, on disk); delete the file of a second blob.
**Expected result**: first chunk verified before status is produced; reading past chunk 40 yields `Err(Corrupt)` and stops (never a complete-looking short body; bytes before the chunk equal source); a Range inside chunks 0..39 succeeds; missing file -> `ContentUnavailable`, no 5xx class error, `blobs.unavailable_at` set.
**Max execution time**: 8 s; **Dependencies**: PostgreSQL, 04

### IT-03-15: Account erase hook
**Summary**: sessions, staged rows, receipts and their blobs are erased in short transactions.
**Traces to**: AC-27, AC-30
**Description**: account with 30 open sessions, 200 staged rows, 5,000 receipts, ProgressTable entries; `begin_deletion`; loop `erase_batch(limit = 100)`.
**Expected result**:
```
Each batch: DELETE ... RETURNING blob_id plus GcQueue::schedule_in_tx(AccountDeleted, Due::Now) in the SAME transaction (invariant: no deleted row without its blob_gc row); no account advisory lock; reservation vanishes with rows (no release_reservation call)
remaining = rows left in upload_sessions + commit_receipts; reaches 0; ProgressTable entries evicted; starts refused with AccountNotActive; put_block, stage_small, commit_upload and commit_batch after begin_deletion -> AccountNotActive with no new node or revision
```
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, 01, 04

### IT-03-17: Crash between blob write and commit
**Summary**: no revision exists until commit; reservation returns at expiry; orphan files are 04's.
**Traces to**: AC-68
**Description**: (a) large path: ack all blocks, drop the service before `commit_upload`; restart; (b) staged path: decorator fails after `write_complete` returns and before the row tx; (c) restart then commit.
**Expected result**: (a) 0 revisions, 0 nodes; `get_upload` after restart shows all blocks (checkpoint) or the missing ones; `commit_upload` still succeeds; if never committed, after TTL the reaper releases `reserved_bytes` to 0. (b) 0 rows, 0 reservation, 1 orphan file (found by the orphan sweeper test in 04, IT-04-14). Invariant: every revision's blob file exists and decrypts.
**Max execution time**: 15 s; **Dependencies**: PostgreSQL, 02, 04, 07a

### IT-03-18: Commit after the parent moved
**Summary**: the node is addressed by id; `parent_id` is a hint.
**Traces to**: AC-70
**Description**: upload for existing file f (base r1) whose folder F is moved to G between `start_upload` and `commit_upload` (and with `parent_id = F` in the request).
**Expected result**: new revision lands on f (same node id) under G; 0 conflict copies; for `Target::New` the parent given is used and a tombstoned parent -> `ParentGone`.
**Max execution time**: 6 s; **Dependencies**: PostgreSQL, 02

### IT-03-19: Effects do nothing after the stamp
**Summary**: statement-log check that 03's effects issue no statement after `FeedStamp::stamp` and no I/O inside the transaction.
**Traces to**: AC-69
**Expected result**: for single and batch commit: `on_begin` lookup, `on_entry` session lock/`mark_committed_in_tx`/delete, `on_batch` receipt insert all precede the stamp CTE; decorators record 0 `BlobReader`/`BlobWriter` calls between `BEGIN` and `COMMIT`.
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, 02

### IT-03-20: Lane races on one session
**Summary**: out-of-order blocks from 4 lanes assemble to the right file.
**Traces to**: AC-7, AC-38
**Input data**: DS-34 slices (4 MiB+1 B to 48 MiB), blocks sent in random order by 4 tasks, 10% duplicated; seed fixed.
**Expected result**: final bitmap complete, `block_hashes` equal the manifest, whole-file hash equals the manifest hash, downloaded bytes equal source; 0 `BlockConflict` for identical duplicates.
**Max execution time**: 60 s; **Dependencies**: PostgreSQL, 04, DS-34

## Performance Tests

### PT-03-1: Block ingest throughput
**Summary**: server-side ingest with fdatasync group commit at the component boundary (no network).
**Traces to**: AC-2, AC-38
**Load scenario**: 4 lanes x 1 session (large file) and 100 sessions x 4 lanes; 2 MiB blocks; 3 minutes; blob volume on XFS.

| Metric | Target (derived from AC-2: 112 MB/s) | Failure Threshold |
|--------|--------|-------------------|
| Single session throughput | >= 250 MB/s | 112 MB/s |
| Ack latency p95 (2 MiB, 100 sessions) | <= 150 ms | 400 ms |
| Error rate (excluding intended `Overloaded`) | 0% | 0% |
**Resource limits**: buffered block bytes <= `max_buffered_block_bytes`; DB connections <= 16. Env C-REF.

### PT-03-2: Constant-time commit
**Summary**: `commit_upload` time does not grow with file size.
**Traces to**: AC-4
**Load scenario**: commit of 1 MiB, 1 GiB, 8 GiB sparse-hybrid files (DS-10 shape), 20 repeats each.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Commit p95 | <= 100 ms | 2 s |
| Spread 1 MiB vs 8 GiB | within 20% | 50% |
**Env**: C-LARGE (50 GiB: NFT-PERF-06).

### PT-03-3: Small-file path
**Summary**: server share of AC-40 (10,000 files, staged + batch).
**Traces to**: AC-40
**Load scenario**: 10,000 `stage_small` (log-uniform 1 KiB to 1 MiB, DS-01) at concurrency 16, then 20 x 500-entry `commit_batch`.

| Metric | Target (derived: half of the 60 s budget, rest is RTT) | Failure Threshold |
|--------|--------|-------------------|
| Total server time | <= 25 s | 45 s |
| `stage_small` p95 | <= 40 ms | 100 ms |
| Batch p95 | <= 1.0 s | 2 s |
**Env**: C-REF.

### PT-03-4: Download and Range
**Summary**: first-byte and 4 KiB range latency; large streaming rate.
**Traces to**: AC-43, AC-2
**Load scenario**: 4 KiB ranges at random offsets of a 1 GiB blob (DS-25), 16 clients, 10,000 reads; one 5 GiB streaming read.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| 4 KiB range p95 (open + first chunk) | <= 4 ms | 5 ms |
| Streaming throughput | >= 250 MB/s | 112 MB/s |
**Env**: C-REF.

### PT-03-5: Memory bound with 100 declared sessions
**Summary**: ProgressTable and buffers stay bounded.
**Traces to**: AC-39, AC-4, AC-51
**Load scenario**: 100 sessions declaring 50 GiB each, one block each in flight; streaming a 5 GiB download.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| ProgressTable memory (100 x 25,600 hashes x 32 B) | <= 80 MB | 100 MB |
| Download RSS growth | <= 16 MiB | 64 MiB |
**Env**: C.

## Security Tests

### ST-03-1: IDOR across accounts
**Summary**: A's upload ids, staged keys, commit ids, nodes and revisions are invisible to B.
**Traces to**: AC-59
**Attack vector**: B calls `get_upload`, `put_block`, `abort_upload`, `commit_upload` with A's `upload_id`; `open_revision_content`/`open_blob_content` with A's ids; same `client_key`, `staged_key` and `commit_id` as A.
**Test procedure**: create A's session/staged/receipt/revision; run each call as B; compare A's state.
**Expected behavior**: `UploadNotFound`/`NotFound`; same keys in B create independent rows.
**Pass criteria**: 0 successful cross-account calls, A state identical, B never receives A's receipt response. **Fail criteria**: any difference.

### ST-03-2: Upload protocol abuse
**Summary**: malformed, oversized and conflicting blocks never corrupt data or reuse nonces.
**Traces to**: AC-7, AC-38
**Attack vector**: wrong hash; body shorter/longer; chunked; same index different bytes; index out of range; `declared_size = 0` on the large path; `declared_size = 2^63`; replay of a block after commit.
**Test procedure**: send each, then read the file bytes via the test volume.
**Expected behavior**: typed errors; the offset of the rejected block still holds the first accepted ciphertext (no rewrite), so the same (key, nonce) never seals two plaintexts.
**Pass criteria**: ciphertext at every offset written once (hash of file region unchanged after rejection). **Fail criteria**: any rewrite. System: NFT-SEC-20.

### ST-03-3: Range abuse
**Summary**: many/overlapping ranges cannot amplify.
**Traces to**: AC-2
**Attack vector**: 10,000 comma-separated ranges; overlapping; `bytes=0-` x 1000 concurrent on a 1 GiB file.
**Test procedure**: call `open_revision_content` with each header.
**Expected behavior**: multi/overlap -> Full single stream (no multipart); the service bounds memory to `blob_read_ahead_chunks`.
**Pass criteria**: RSS growth <= 64 MiB with 1000 concurrent streams started; 0 multipart. **Fail criteria**: any amplification. System: NFT-SEC-18.

### ST-03-4: Hostile content types
**Summary**: DS-17 hostile files are decided by bytes, never by name.
**Traces to**: AC-47
**Attack vector**: html/svg-with-script/xml/js/pdf/GIF89a-HTML polyglot/SVG `onload`; positives jpg/png/webp/gif/mp4.
**Test procedure**: upload each under benign names (`a.jpg`) and hostile names (`a.html`), commit, download.
**Expected behavior**: `Inline` only for the 7 allowlisted sniffed types; every other `Attachment`.
**Pass criteria**: 0 inline renders of non-allowlisted types (12/12 hostile Attachment, 5/5 positives Inline). **Fail criteria**: any inline hostile.

### ST-03-5: Quota and disk DoS
**Summary**: 100 sessions of 50 GiB are refused beyond the cap/quota and free space never drops below the floor.
**Traces to**: AC-39
**Attack vector**: DS-26 shaped: 100 `start_upload` declaring 50 GiB concurrently from one account and from 20 accounts; disk filler leaving floor + 1 GiB.
**Test procedure**: start all, then upload blocks until refusals.
**Expected behavior**: per account <= 8 sessions; others `SessionCapReached`/`QuotaExceeded`; `FreeSpaceFloor` stops block writes before the floor.
**Pass criteria**: free bytes >= floor throughout; reserved <= quota. **Fail criteria**: floor crossed. System: NFT-RES-LIM-01, NFT-RES-05.

## Acceptance Tests

### AT-03-1: Interrupted upload resumes
**Summary**: a user resumes a 100 MiB upload after a drop.
**Traces to**: AC-3, AC-4
**Preconditions**: account with quota; same `client_key`.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | start, send 40 MiB, drop connections | session persists |
| 2 | start again with same `client_key` | same `upload_id`, `need_blocks` = missing blocks only |
| 3 | send the rest, `commit_upload` | file hash equals source; re-sent <= 10 MiB |

### AT-03-2: Folder of small files with a retried batch
**Summary**: 500 staged files committed with a lost response.
**Traces to**: AC-40
**Preconditions**: DS-01 subset, batch `commit_id` persisted.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `stage_small` x 500, `commit_batch` | 500 Committed |
| 2 | resend the identical batch | stored response, no duplicates |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-01 (S) | many-small files | generated (seed 20261103) | 1.5 GB (500-file subset for component runs) |
| DS-17 (S) | hostile upload types | generated | < 5 MB |
| DS-23 | files at 2 MiB-1, 2 MiB, 2 MiB+1 | generated | small |
| DS-25 | 1 GiB dense file | generated | 1 GiB |
| DS-34 (S) | 100 multi-block slices with manifests | generated | 2.5 GiB (subset of 10) |
| DS-10 | 50 GiB sparse-hybrid (PT-03-2 uses 8 GiB slices) | generated | sparse |

**Setup procedure**: (1) one `postgres:18@sha256:<pin>` per test binary; migrate as `sunnycloud_migrator` into template `tmpl`; per test `CREATE DATABASE ... TEMPLATE tmpl`; component runs as role `sunnycloud`; (2) per test temp blob root (XFS loop volume for C-REF runs); test master key (DS-33); manual `Clock`; ms-scale TTL/checkpoint/reap config; (3) accounts through the real 01 hooks; quota set through `QuotaAdmin`.

**Teardown procedure**: drop database and blob directory; leak check: blob root file count equals committed + derived rows after each test, container removed at binary end.

**Data isolation strategy**: database and blob root per test; fault decorators are per-test objects; large generated data is read-only and shared from `/data`.
