# 03 Transfer

## 1. High-Level Overview

**Purpose**: Moves file bytes in and out: upload sessions with a received-bitmap, 2 MiB blocks over up to 4 lanes, single-request staged uploads, single and batch commit, and download with Range streaming.

**Architectural Pattern**: Application service behind traits. Per-session in-memory progress with DB checkpoints. 03 never opens the commit transaction: it calls the `TreeCommit` port and supplies `CommitEffect`s that run inside 02's transaction, so there is one writer of the tree.

**Single responsibility**: "Get verified bytes into a durable blob and hand a ready blob to the tree; stream a stored blob back." It does not decide names, conflicts, revisions, quota policy or encryption.

**Upstream dependencies**: `contracts` ports only (see Ports); helpers 06_names, 05_ids_hashing, 03_clock, 01_config, 02_error, 07_db.
**Downstream consumers**: through ports only; see Ports.

## 2. Scope

**In**: session lifecycle, resume, block receive and verify, in-memory progress and checkpoint, in-flight caps, staged small uploads (including zero-byte files), commit and batch commit orchestration (inputs, receipts, sniffing), session reaper, receipt janitor, download and Range semantics, inline-vs-attachment decision (AC-47), the account-erase hook for sessions, staged rows and receipts.
**Out**: node names, conflict copies (the server-assigned name for a new file too), edit-beats-delete, revision rows, the commit transaction, feed counter (02_tree); quota limits and usage (07a); encryption, fsync, GC (04); thumbnails (08); HTTP headers and status mapping (11); share token logic (06).

## 3. Internal Interfaces

All operations take a resolved `AccountId` (from the delivery layer's auth), never a credential. Errors are typed enums; none is swallowed.

### Interface: UploadService

| Method | Input | Output | Async | Errors |
|---|---|---|---|---|
| `start_upload` | `StartUpload{parent_id, name, declared_size, client_key, content_hash?, base_revision?, client_meta?}` | `Started{upload_id, need_blocks, block_size=2MiB, lane_limit, expires_at}` or `NothingToSend{revision}` (the node already holds a revision with `content_hash`, 0 bytes) | Yes | `InvalidName`, `ParentNotFound`, `NotAFolder`, `SizeTooLarge`, `QuotaExceeded`, `SessionCapReached`, `FreeSpaceFloor`, `ClientKeyMismatch`, `AccountNotActive` |
| `get_upload` | `upload_id` | `UploadState{need_blocks, received_count, expires_at}` | Yes | `UploadNotFound` (also expired) |
| `put_block` | `upload_id`, `index`, `expected_hash` (BLAKE3), `body: ByteStream`, `length` | `BlockAck` (idempotent for a repeat of the same hash) | Yes | `UploadNotFound`, `IndexOutOfRange`, `LengthMismatch`, `HashMismatch`, `BlockConflict` (acked block with other bytes or in flight), `Overloaded` (global buffer, lane limit or account in-flight cap), `StorageFull`, `Storage` |
| `abort_upload` | `upload_id` | `()` releases quota and file | Yes | `UploadNotFound` |
| `stage_small` | `staged_key` (client UUID), `expected_hash` (omitted when `length = 0`), `body`, `length` in `0..=2 MiB` | `Staged{staged_key, size}` (idempotent by key) | Yes | `QuotaExceeded`, `StagedCapReached`, `HashMismatch`, `LengthMismatch`, `ClientKeyMismatch`, `FreeSpaceFloor`, `Overloaded`, `AccountNotActive`, `Storage` |

Notes:
- `start_upload` runs the name rules (helper 06), checks the parent through `TreeReader::get_node` (`ParentNotFound`, `NotAFolder`) and a cheap `TreeReader::find_live_node` check for the `NothingToSend` shortcut, checks the free-space floor, plans the blob (`BlobWriter::plan`), then in one short tx reserves quota (`QuotaLedger::reserve`: one atomic UPDATE that also takes the quota row lock), checks the open-session cap (the lock held since `reserve` serialises concurrent starts; a refusal rolls the reservation back), and inserts `blobs` (`uploading`) and `upload_sessions`; the file is created after the tx and, if that fails, a compensating short tx removes the rows and releases the reservation. A repeat with the same `(account, client_key)` returns the existing session (AC-50 resume). A repeat whose declared size differs replaces nothing: `ClientKeyMismatch`. Large path means `declared_size > 0`; a zero-byte file goes through `stage_small`.
- The file hash is optional at start (only for the `NothingToSend` shortcut). `put_block` carries `X-Blake3`; the server buffers the 2 MiB block (global semaphore `max_buffered_block_bytes`), hashes it, rejects a mismatch **before** any write, then `write_block` and `await_durable`. Only then the bit is set in memory (and `block_hashes[index]` filled) and 204 is returned.
- **In-flight caps.** Before buffering a block, `put_block` checks the session's in-flight bitset against `lane_limit` (the value returned at start, `max_lanes`) and the account's total in-flight blocks (including staged writes) against `max_inflight_blocks_per_account`; either excess is `Overloaded` (503 + `Retry-After`) and nothing is read from the body.
- **Staged small upload** (decision 11), in this order: (1) look up `(account, staged_key)`: an existing row answers `Staged` without writing anything (same size and hash; otherwise `ClientKeyMismatch`); (2) pre-check the staged-file cap (`StagedCapReached`), the account in-flight cap and the quota (`QuotaAdmin::view`: free room at least `length`), so a refused request writes nothing; the authoritative `reserve` follows in step 4; (3) write first (`write_complete`; for `length = 0` an empty blob with no block); (4) one tx: `QuotaLedger::reserve`, `blobs` `uploading`, and a completed `upload_sessions` row (`block_count = ceil(length / 2 MiB)`, so 0 for an empty file and 1 otherwise, `declared_size = length >= 0`, received bits already set). A refusal or failure after the write discards the file (compensating `BlobWriter::discard`). Staged rows count against the staged-file cap (`max_staged_per_account`), not the large-session cap, and the reaper expires them like sessions.

### Interface: CommitService

Both methods take `CommitActor{account_id, device_label}` (11 fills the label from the principal; the label names the losing device in conflict copies) and pass it to `TreeCommit::publish`.

| Method | Input | Output | Async | Errors |
|---|---|---|---|---|
| `commit_upload` | `CommitUpload{upload_id, parent_id, name, base_revision?, blocklist, client_mtime?, client_meta?, import_hint}` | `Committed{node_id, revision_id, conflict_copy?}` | Yes | `NeedBlocks(list)`, `BlocklistMismatch`, `UploadNotFound`, `InvalidName`, `EditedSince`, `ParentGone`, `ParentNotFound`, `NotAFolder`, `AccountNotActive` |
| `commit_batch` | `BatchCommit{commit_id, entries[<= batch_max_entries]}` entry = `{staged_key, parent_id, name, base_revision?, client_mtime?, client_meta?, import_hint}` | `BatchResult{per_entry: Committed \| EntryError}` | Yes | `BatchTooLarge`, `CommitIdReused` (same id, different request hash), `AccountNotActive` |

Target choice: with `base_revision` the node is the one that revision belongs to (`TreeReader::get_revision`), so the commit is `Target::Existing` and an edit beats a delete; without it the commit is `Target::New{parent_id, name}`. A taken name on a new file is not an error: the server names the new file with the conflict-copy pattern and answers `Committed{conflict_copy}` (02 §5); there is no client retry on `name_conflict` for new files.

Flow: validate and sniff first (outside any tx): compare `blocklist` with the in-memory or checkpointed `block_hashes`, derive the whole-file hash from the 32-byte hashes (no file bytes read), read the head via `BlobReader::read_head` and sniff the media type from bytes only (client-declared type and file name are ignored; unknown is NULL and is served as an attachment; resolves gap G-10). Then call `TreeCommit::publish` once (one entry for a single commit, up to `batch_max_entries` for a batch), passing the list of `CommitEffect`s. The tree owns the transaction, the account lock, savepoints per entry, the quota calls (`adjust_used` with the delta, `release_reservation` with the `reserved_bytes` that 03 reads from the session row and puts into `NewRevision`) and the counter bump as the last statement. `NewRevision` and the batch entry carry `import_hint`; `PublishEntry.vault` is false in v1.0 (v1.2: set from the commit request, see 20). 08 receives `CommitContext{…, import_hint, vault}` and plans no jobs for a vault file. Transfer supplies these effects:
1. `ReceiptEffect.on_begin` (batch only): inside the transaction, under the account lock that `publish` already holds, look up `commit_receipts(account, commit_id)`. Same `request_hash`: `Replay(stored response)`, nothing is applied. Different hash: `EffectError::Batch(CommitIdReused)`. Because the lookup and the later insert are both under the lock, two identical batches cannot both apply, and nothing relies on a unique violation inside a PG transaction.
2. `BlobCommitEffect.on_entry`: `SELECT … FROM upload_sessions … FOR UPDATE` for the entry's session or staged row. Zero rows (reaped, aborted, already committed) is `EffectError::Entry(UploadNotFound)` and that entry's savepoint, including the quota release 02 did for it, rolls back. Otherwise `BlobRegistry::mark_committed_in_tx` and delete the row.
3. `ReceiptEffect.on_batch` (batch only): receives the outcome of every entry, failed ones included, and inserts `commit_receipts(account, commit_id, request_hash, response)`.
4. Effects injected by the composition root (08 enqueues media jobs; nothing else is hard-wired in 03).
Receipts exist for batch commits only. The large path (more than 2 MiB) is idempotent without a receipt: a lost commit response is healed by `start_upload` with the same `client_key` and file hash, which answers `NothingToSend{revision}`.

**`client_key`** (opaque to the server, at most 256 bytes, unique per account; a different key means a new session): web `web:` + SHA-256 of parent id, name, size and `lastModified`; desktop `desk:` + volume id, file id, size and mtime of the local file; Android and iOS `asset:` + device id, MediaStore or Photos asset id and the file hash (the inputs of `derive_commit_id`). A file that changed on disk therefore gets a new key and a fresh session (AC-50). `staged_key` is a client UUID. **Batch retry rule**: the client persists the batch `commit_id` together with the list of `staged_key`s and entries before the first send and reuses all of it on every retry, so the request hash is identical and the stored receipt answers (FT-N-38). Entries that failed are re-sent in a new batch with a new `commit_id`.

### Interface: DownloadService

| Method | Input | Output | Async | Errors |
|---|---|---|---|---|
| `open_revision_content` | `account_id`, `RevisionRef \| NodeRef(current)`, `retained_ok: bool`, `Range?`, `IfRange?`, `IfNoneMatch?` | `Download{status: Full\|Partial\|NotModified, total_size, range, etag, media_type, disposition: Inline\|Attachment, suggested_name, body: PlaintextStream}` | Yes | `NotFound`, `RangeNotSatisfiable`, `ContentUnavailable` (blob missing or suspect), `Storage` |
| `open_blob_content` | `account_id`, `BlobRef{blob_id, size, media_type, key material}`, same options | same | Yes | same |

- `NodeRef(current)` is resolved with `TreeReader::resolve_live_file` (a folder or `Gone` is `NotFound`). `RevisionRef` uses `TreeReader::get_revision`. **Retained non-current revisions are not downloadable by users** (operator restore only): unless `retained_ok` (11 sets it only for an admin principal), a revision that is not its node's current one is `NotFound`.
- Exactly one byte range is honoured; a multi-range or overlapping header is ignored (full 200). Out of bounds is `RangeNotSatisfiable` (416). ETag is the strong blob id (blobs are immutable). `HEAD` uses the same call and drops the body.
- Disposition: `Inline` only for image/jpeg, png, gif, webp, avif and video/mp4, webm (fixed list, AC-47); everything else `Attachment`. The response is a decision; 11 turns it into headers (`nosniff`, CSP sandbox).
- The stream is the 04 reader: first chunk verified before headers are produced; a later `Corrupt` ends the stream with an error, never a complete short body. Blob missing maps to `ContentUnavailable` (404 with a distinct code), never 5xx.
- Account scoping: the revision or blob row is looked up with `account_id`; a foreign id is `NotFound` (AC-59).

### Interface: Janitors (`Janitor` port: bounded `run_cycle`; scheduled by the `sunnycloud` binary, decision 16)

`SessionReaper::run_cycle`: sessions with `expires_at <= now` (rolling TTL from the last checkpoint). Per batch, one short tx: `DELETE FROM upload_sessions … RETURNING blob_id, reserved_bytes`, then `QuotaLedger::release_reservation` for exactly the returned amount and `BlobRegistry::remove_row_in_tx`; files are discarded after the commit. `abort_upload` does the same for one row. A reservation is released by whoever deletes the row, so reaper, abort and commit never release it twice (a commit that lost the row gets `UploadNotFound` and rolls back its own release). `ReceiptJanitor::run_cycle`: delete receipts older than `commit_receipt_ttl`. `ProgressTable::flush_idle`: write checkpoints for sessions idle longer than the checkpoint interval and evict.

`ReservationSource::open_reserved_bytes(account) -> i64` (for the caller of 07a `recompute`, 12 doctor or 07): sum of `reserved_bytes` of the account's open sessions and staged uploads.

### AccountLifecycleHook (implemented here; driven by 07's `DeletionPurger`)
`initialize(tx, account)` does nothing. `erase_batch(account, limit) -> EraseProgress{remaining}`: in one short tx per batch (no account advisory lock) `DELETE FROM upload_sessions … RETURNING blob_id` for open sessions and staged rows, and `GcQueue::schedule_in_tx(tx, blob_id, AccountDeleted, Due::Now)` for each blob in the same transaction; then `commit_receipts` in batches. Reservations vanish with the rows (07a erases the `quota` row, so no `release_reservation` is called). The in-memory `ProgressTable` entries of the account are evicted. `remaining` = rows left in `upload_sessions` and `commit_receipts` for the account. New starts are already refused (`AccountNotActive`).

## Ports (crate `contracts`)

| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `UploadService`, `CommitService`, `DownloadService` | 11 | all transfer routes; share and derived downloads (11 composes 06 `authorize_content` or 08 `BlobRef` with `DownloadService`) |
| Provides | `AccountLifecycleHook` impl | 01 (initialize), 07 (erase_batch) | sessions, staged rows, receipts, reservations at account deletion |
| Provides | `ReservationSource::open_reserved_bytes(account)` | 12, 07 (callers of 07a `recompute`) | drift check |
| Provides | `Janitor` (`SessionReaper`, `ReceiptJanitor`, `ProgressTable::flush_idle`) | 12 scheduler | expiry and checkpoints |
| Provides | `CommitEffect` impls (`BlobCommitEffect`, `ReceiptEffect`) | 02 (passed to `TreeCommit::publish`) | receipt lookup, blob state, session row, receipts |
| Requires | `TreeCommit`, `TreeReader` | 02 | publish; parent, existing-target and revision-to-node checks; `resolve_live_file` for downloads |
| Requires | `QuotaLedger` (`reserve`, `release_reservation`), `QuotaAdmin::view` | 07a | 03 reserves at start and releases on abort and expiry; 02 releases at commit inside its transaction; `view` for the staged pre-check |
| Requires | `BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog`, `GcQueue` | 04 | write, read, rows, queue blobs at account erase |
| Requires | `CommitEffect` (extra, injected) | 08 | media job enqueue |

## 4. External API Specification (owned resources; handlers live in 11)

| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/v1/uploads` | POST | session or device | Start or resume (idempotent by `client_key`) |
| `/api/v1/uploads/{upload_id}` | GET, DELETE | session or device | State and `need_blocks`; abort |
| `/api/v1/uploads/{upload_id}/blocks/{index}` | PUT | session or device | One 2 MiB block; `X-Blake3`, exact `Content-Length`; 204 after fdatasync |
| `/api/v1/staged-uploads/{staged_key}` | PUT | session or device | Single request up to 2 MiB, zero bytes allowed (no `X-Blake3`); idempotent |
| `/api/v1/commits` | POST | session or device | Publish one large upload (blocklist in body) |
| `/api/v1/commits:batch` | POST | session or device | Fixed by the solution (colon form is the documented exception to kebab nouns) |
| `/api/v1/nodes/{node_id}/content`, `/api/v1/revisions/{revision_id}/content` | GET, HEAD | session or device | Range, 200/206/304/416; the revision route serves current revisions only for non-admin principals |

Errors: problem+json from 11 with the stable codes and statuses of helper 02: `quota_exceeded` 507, `free_space_floor` 507, `size_too_large` 413, `session_cap_exceeded` 409, `staged_cap_reached` 409, `commit_id_reused` 422, `batch_too_large` 422, `blocklist_mismatch` 422, `account_not_active` 403, `parent_not_found` 404, `not_a_folder` 422, `parent_deleted` 410, `block_hash_mismatch` 422, `block_length_mismatch` 422, `block_conflict` 409, `need_blocks` 409, `edited_since` 409, `content_unavailable` 404, `range_not_satisfiable` 416, `upload_session_gone` 410, `overloaded` 503. `name_conflict` does not occur on commit (new files get a conflict-copy name).

## 5. Data Access

| Query | Frequency | Hot path | Index |
|---|---|---|---|
| Insert blob+session; reserve quota | per upload | Yes | PK, `(account_id, client_key)` unique |
| Lock session row at commit (`FOR UPDATE`) | per commit entry | Yes | PK / `(account_id, client_key)` |
| Checkpoint `received`, `block_hashes` | every 8 MiB or 1 s per session | Yes | PK |
| Commit effects (inside 02's tx; no statement of 03 touches `nodes`, `revisions` or `quota`) | per commit | Yes | PK |
| Receipt lookup (under the account lock, in the commit tx) / insert | per batch | Yes | `(account_id, commit_id)` PK |
| Reaper scan | every few minutes | No | `(expires_at)` |

**State**: `ProgressTable`, an in-memory map `upload_id -> {received bitset, in_flight bitset, block_hashes}` guarded per session. Justification against stateless: acks arrive at about 56 blocks/s for 100 sessions and rewriting an up to 800 KiB hash array per ack is the WAL cost the data model already rejected; the DB checkpoint is the durable state and the table is rebuilt lazily from it (loading, never rehydrating at start). Consequence (decision 12): all lanes of one session must reach the same API process, so v1.0 runs one API process; replicas are unsupported for uploads. Deployment docs state it and the compose file has no `replicas` setting.
Memory bound: 100 sessions x 800 KiB hashes = 80 MB worst case; buffered block bytes are bounded by `max_buffered_block_bytes` and the in-flight caps.

Storage estimates: `upload_sessions` open rows under 1,000; `commit_receipts` about 10 k/day x 48 h x 2 KB = 40 MB; no caching of downloads.

## 6. Configuration

| Key (env) | Default | Notes |
|---|---|---|
| `SUNNYCLOUD_UPLOAD_SESSION_TTL` | 24h rolling | test 60s |
| `SUNNYCLOUD_MAX_DECLARED_SIZE` | 50GiB | above it `SizeTooLarge`; 0 bytes is allowed on the staged path |
| `SUNNYCLOUD_MAX_OPEN_SESSIONS` / `SUNNYCLOUD_MAX_STAGED` | 8 / 1000 per account | AC-39 |
| `SUNNYCLOUD_CHECKPOINT_BYTES` / `SUNNYCLOUD_CHECKPOINT_INTERVAL` | 8MiB / 1s | |
| `SUNNYCLOUD_MAX_LANES` | 4 | returned as `lane_limit`; enforced per session on `put_block` |
| `SUNNYCLOUD_MAX_INFLIGHT_BLOCKS_PER_ACCOUNT` | 32 (`max_open_sessions` x `max_lanes`) | in-flight blocks of one account, large and staged |
| `SUNNYCLOUD_BATCH_MAX_ENTRIES` | 500 | |
| `SUNNYCLOUD_COMMIT_RECEIPT_TTL` | 48h | |
| `SUNNYCLOUD_MAX_BUFFERED_BLOCK_BYTES` | 1GiB | backpressure: `Overloaded` maps to 503 + Retry-After (also for the lane and account caps) |
| `SUNNYCLOUD_SESSION_REAP_INTERVAL` / `..._RECEIPT_JANITOR_INTERVAL` | 1m / 1h | |

Block size 2 MiB is a protocol constant, not configuration.

## 7. Security Notes

- Hash is verified before the first byte is written; a corrupt body never reaches disk (also avoids nonce reuse, see 04 risks).
- Body length must equal the expected block length (`min(2 MiB, remaining)`); longer, shorter or chunked-without-length is rejected (NFT-SEC-20 step 6).
- All lookups carry `account_id`; revoked or deleting accounts are rejected at start, put and commit (`AccountNotActive`).
- Declared size above the cap is refused at start; reservation happens before any file exists. `stage_small` checks key, cap and quota before it writes a byte.
- No file names in logs or errors. Content is always decided by sniffed type, never by the name.

## 8. ACs Implemented

| AC | Release | Part |
|---|---|---|
| AC-2, AC-4 | v1.0 | Streaming without per-file copies; constant-time commit (hash roll-up only) |
| AC-3 | v1.0 | Resume re-sends <= 4 lanes x 2 MiB plus uncheckpointed 8 MiB |
| AC-7 | v1.0 | Per-block BLAKE3 on receive; whole-file hash from block hashes |
| AC-38 | v1.0 | Ack after durable |
| AC-39 | v1.0 | Reserve at start, session cap, floor |
| AC-40 | v1.0 | Staged upload (zero bytes included) and batch commit with receipts checked under the account lock |
| AC-42 | v1.0 | Name rules at start (final check in 02) |
| AC-47 | v1.0 | Inline allowlist decision |
| AC-50 | v1.0 | `client_key` resume, restart on changed key |
| AC-51 | v1.0 | Range and bounded memory on download |
| AC-43 (read side) | v1.0 | Range 4 KiB p95 via 04 |

**Interaction ACs folded in**
- **AC-68**: commit only after all bits set from fdatasynced blocks; staged files are synced before the row exists; reaper releases reserved quota at session expiry, once, by deleting the row it releases for; orphan sweep is 04's.
- **AC-69**: the single commit transaction is 02's; 03's effects add no statement after the stamp and perform no reads or I/O inside the tx (sniffing and hash roll-up precede it).
- **AC-70**: a commit names the node by id; if its parent was moved meanwhile the revision lands in the moved folder (tree locks the node by id, not path). 03 passes `parent_id` as a hint only.
- **AC-74**: reserve and release go through the `QuotaLedger` port; at commit 02 applies the usage delta (`adjust_used`) and `release_reservation(reserved_bytes)` inside the commit transaction; reaper and abort release exactly `reserved_bytes` of the row (no recomputation), keeping usage equal to the sum of live revisions.
- **AC-77** (download side): the share download calls `open_revision_content` on the current revision 06 returned; a deleted node resolves to `NotFound`.
- **Account deletion** (AC-27, AC-30): sessions, staged rows, receipts and their blobs are erased by the hook (blobs queued with `Due::Now` in the same transaction).

## 9. Tests

FT-P-01..06, FT-P-67, FT-P-68, FT-P-42, FT-P-39, FT-N-01, FT-N-02, FT-N-03, FT-N-11, FT-N-38, FT-N-40, FT-N-41, NFT-PERF-02..07, NFT-PERF-27, NFT-PERF-28, NFT-RES-01..06, NFT-RES-22, NFT-RES-LIM-01, NFT-SEC-18, NFT-SEC-19, NFT-SEC-20, NFT-SEC-21. New Step 5 scenarios for AC-68 (kill between blob write and commit), AC-69 (50 concurrent commits, feed exactly once), AC-70 (move vs edit), AC-74 (1,000 concurrent commits and deletes keep usage = sum). Unit: bitmap/lane races, duplicate-block rules, receipt race, range math.

## 10. Acceptance for the Component

Resume re-sends <= 10 MiB; 0 zero-filled committed files after kill -9; 10,000 small files in <= 60 s at 20 ms RTT; commit time within 20% between a 1 MiB and a 50 GiB file; 100 sessions of 50 GiB are refused beyond cap and quota; 0 multipart responses; coverage 90% on put_block, commit and reaper paths.

## 11. Risks

| Risk | Mitigation |
|---|---|
| In-memory progress breaks with two API replicas | One API process in v1.0; documented; checkpoint makes a restart safe |
| Commit effects depend on the `CommitEffect` contract | Defined in `contracts` (02 section 2); contract tests run 02's real commit with 03's real effects, including reaper vs commit on one session |
| Buffering 2 MiB per lane under many sessions | Global byte semaphore, 503 + Retry-After |
| Sniffing reads the head outside the tx | Head read is immutable data; a failed read is `ContentUnavailable` before the tx |
| Staged files never committed | Reaper at TTL; separate cap |

## 12. Dependency Graph

After: `contracts` and helpers. At compile time depends on no component crate. Integration tests need real 02, 04 and 07a behind the ports. Blocks: 11_http_api transfer routes, client components.

## 13. Logging

ERROR: storage I/O failure, commit transaction failure other than typed conflicts. WARN: rejected block (hash mismatch), quota/session-cap/floor refusal, overload. INFO: session created/committed/expired (ids only), batch committed with entry counts. No per-block lines; metrics per `observability.md` Transfer table.
