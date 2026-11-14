# Contract: upload-service (UploadService and ReservationSource behaviour)

**Component**: 03_transfer
**Producer task**: E-14/c04 — `_docs/02_tasks/todo/B03-20_transfer_start_upload.md` (contributors: E-14/c05 `put_block`, E-14/c06 `get_upload`/`abort_upload`, E-14/c07 `stage_small`, E-14/c11 `ReservationSource`; rule from spike E-14/c02); keys assigned at Step 4
**Consumer tasks**: E-14/c13, E-14/c16 (this batch); E-21/c13 (11 handlers), E-22 (doctor, `recompute` caller), E-17 (07 `recompute` caller), E-23/E-26/E-27 through the HTTP API, resolved at Step 4
**Version**: 1.7.0
**Status**: draft (the `NothingToSend` rule is recorded; frozen when the producer task passes review)
**Last Updated**: 2026-11-14

## Purpose

Getting verified bytes into a durable blob: resumable large-upload sessions over 2 MiB blocks and up to 4 lanes, and single-request staged uploads of up to 2 MiB. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 03 transfer); this file freezes the behaviour.

## Shape

Signatures: part 2 (`UploadService`, `ReservationSource`). Behaviour per method:

| Method | Behaviour | Errors |
|---|---|---|
| `start_upload` | Order: size cap, name rules, parent check, `NothingToSend` shortcut, free-space floor, then one short transaction (quota reserve, session cap, blob and session rows), file created after it, compensation on failure. Same `(account, client_key)` returns the existing session | `InvalidName`, `ParentNotFound`, `NotAFolder`, `SizeTooLarge`, `QuotaExceeded`, `SessionCapReached`, `FreeSpaceFloor`, `ClientKeyMismatch` (same key, other declared size), `AccountNotActive` |
| `get_upload` | `need_blocks` (blocks not received; in-flight blocks are listed), `received_count`, `expires_at`, from the in-memory entry. When this process has no entry (after a restart) the first access (`get_upload` or `put_block`) reads the open, unexpired row in one short transaction, takes the blob's plan from `BlobCatalog::get_uploading_plan`, reopens the file with `BlobWriter::open_for_write` and builds the entry from `received` and `block_hashes`: blocks acknowledged after the last checkpoint are needed again. An entry another request loaded meanwhile wins | `UploadNotFound` (also expired, foreign, committed); `Storage` (file cannot be reopened); `Unexpected` (blob row not `uploading`) |
| `put_block` | Order: in-memory session, loaded lazily as for `get_upload` when this process has none (absent, foreign or past `expires_at`: `UploadNotFound`), index, declared length `min(2 MiB, remaining)`, duplicate rules (received with the same hash: `BlockAck` with no read and no write; another hash for a received or in-flight index: `BlockConflict`; same hash in flight: `Overloaded`), lane cap, account cap (`max_inflight_blocks_per_account`, shared with staged writes) and buffer cap (`max_buffered_block_bytes`), all before any body byte is read; the body is read in full and its length and BLAKE3 checked before `write_block`; the bit is set after `await_durable`. Every `checkpoint_bytes` or `checkpoint_interval` one short transaction runs `AccountGuard` (owner 2026-11-13) and rewrites `received`, `block_hashes`, `last_checkpoint_at` and `expires_at`; it never writes a committed or expired row (0 rows: the entry is dropped, `UploadNotFound`). No database call otherwise (besides the lazy load); no transaction across the body read or the sync | `UploadNotFound`, `IndexOutOfRange`, `LengthMismatch`, `HashMismatch`, `BlockConflict`, `Overloaded`, `StorageFull`, `Storage`, `AccountNotActive` (from the checkpoint) |
| `abort_upload` | One short transaction: `AccountGuard`, `DELETE … RETURNING blob_id, reserved_bytes` of the open, unexpired row (an expired row is the reaper's), `QuotaLedger::release_reservation` for exactly that amount, `BlobRegistry::remove_row_in_tx`. After COMMIT the progress entry is evicted and the file discarded (best effort: a failure is logged and the file is left to 04's orphan sweeper); `upload_sessions_total{result=aborted}`. Needs no progress entry. A repeat finds no row and releases nothing | `UploadNotFound` (also expired, foreign, committed), `AccountNotActive` |
| `stage_small` | A declared `length` above 2 MiB is `LengthMismatch` before anything else (proposal AZ-3737, pending the E-14 owner). Then one short transaction: `AccountGuard`, the key lookup (the row's `client_key` is the staged key's hyphenated text; an uncommitted staged row of the same size, hash and received block answers `Staged` with no body read and no write; any other row under the key, a large session included, is `ClientKeyMismatch`) and the staged cap (`max_staged`: open rows with at most one block, all received). Then the account in-flight cap (`Overloaded`, 0 body bytes read) and the free room by `QuotaAdmin::view`. The body is read in full and checked for length and BLAKE3 before `write_complete` (04 refuses below the free-space floor before it creates the file); `expected_hash` may be omitted, or be the hash of no bytes, only for `length = 0`; bytes without a hash are `HashMismatch`. Then one transaction: `AccountGuard`, `QuotaLedger::reserve` (its row lock serialises the account's stages, so the staged cap is checked again exactly), `blobs` `uploading` and a completed row (`block_count` 0 with empty `received`/`block_hashes`, or 1 with its bit and hash set, `reserved_bytes = declared_size`, `expires_at = now + upload_session_ttl`). A refusal or failure of that transaction discards the file (best effort, logged); a concurrent stage that inserted the same key first answers by its row | `QuotaExceeded`, `StagedCapReached`, `HashMismatch`, `LengthMismatch`, `ClientKeyMismatch`, `FreeSpaceFloor`, `Overloaded`, `AccountNotActive`, `Storage` (`StorageFull` when the volume is full, as for `put_block`) |
| `ReservationSource::open_reserved_bytes(account)` | Sum of `reserved_bytes` of the account's rows in one short account-scoped read: open sessions and staged rows, expired ones not yet reaped included (07a holds their reservation until the reaper releases it); a committed record holds 0 | `Unexpected` |

### Expiry and janitors (E-14/c11, AZ-3756)

- One expiry rule: a session or staged row is gone once `now >= expires_at`, with `expires_at = last_checkpoint_at + upload_session_ttl` (rolled by every checkpoint). `get_upload`, `put_block`, the lazy load, `abort_upload`, `commit_batch` and the reaper apply it; `start_upload` and `stage_small` apply it to the row under their key: an expired row (open, staged or committed) answers nothing and is reaped in the start or stage transaction (reservation released, blob row removed, file discarded after COMMIT), then the key is used anew. Before this, an expired unreaped staged row still answered `Staged` and an expired open session was resumed.
- Lock order: every path that deletes a row and releases its reservation (`publish` in 02, the reaper, `abort_upload`, the key reap) takes the account advisory lock first, so a commit racing a reap or an abort waits instead of deadlocking (batch 61 warning).
- `SessionReaper` (`Janitor`, `session_reaper`): lists the accounts with expired rows in system scope (`janitor_repo`, pages of 100), then per account, in batches of 100, one short account-scoped transaction: advisory lock, `DELETE … RETURNING` (row lock waits for a running checkpoint and re-checks the rolled expiry), `release_reservation` for exactly the returned sum, `remove_row_in_tx` for each open row. After COMMIT the progress entries are evicted (`upload_sessions_active` falls), the files of open rows discarded (best effort), and `upload_sessions_total{result=expired}` counts each large-path session (the session-cap predicate; a one-block session with its block counts as staged). Committed records are deleted without touching blob or quota. `processed` = rows deleted. No leader: a second reaper waits for the advisory lock and finds nothing.
- `ReceiptJanitor` (`receipt_janitor`): deletes receipts with `created_at < now - commit_receipt_ttl` in system-scope batches of 1000 (`FOR UPDATE SKIP LOCKED`); a later batch with a deleted `commit_id` is a new batch.
- `IdleFlush` (`progress_flush`; the description's `ProgressTable::flush_idle`, on the service because the table has no database handle): an entry with no block in flight and none acknowledged for `checkpoint_interval` writes its unsaved `received` and `block_hashes` as a checkpoint (no `AccountGuard`: no client request) and is evicted; an entry with nothing unsaved is evicted without a write. A block that starts between the idle check and the eviction lands in the evicted entry; at worst its bit misses the row and the client resends it, as after a restart.
- Scheduling and registration of the three janitors are E-22/c06.

### `NothingToSend` rule (decided 2026-11-11, AZ-3732)

Applies only when `StartUpload.content_hash` is present; without it `start_upload` never answers `NothingToSend`. It runs after the name rules and the parent check and before the free-space floor, and reads the tree only (no write, no reservation). The node is matched by `name_key` through `TreeReader::find_live_node(account, parent_id, name)`, never by a case-sensitive string compare, so a retry with another case of the name still finds it.

| Case | Answer |
|---|---|
| A live file at (parent, name) whose current revision has the same `content_hash` | `NothingToSend{revision}` = that current revision (0 bytes, nothing reserved). This is the heal after a lost commit response of a new file |
| A live file at (parent, name) whose current revision has another hash | `Started`; the commit decides (replace, or a conflict copy when the base is stale) |
| Only a tombstoned file at the name | `Started`; tombstones never match (`find_live_node`); the commit may revive |
| A live folder at the name | `Started`; no error: a taken name is settled at commit (`CreatedAsCopy`), as for any other name clash |
| No node at the name | `Started` |
| `base_revision` given | The node is the one that owns `base_revision` (by id, AC-70), not the one at the name. If it is live and its current revision has the same hash: `NothingToSend{revision}` = that current revision; otherwise `Started`. A `base_revision` that is unknown, foreign or whose node is gone is `Started`, never an error |

Committed key (owner decision 2026-11-13, AZ-3949, option a): before the cases above, a `client_key` whose session committed within `upload_session_ttl` answers `NothingToSend{revision}` = the revision that commit created, whatever the name holds now (taken at commit, replaced, moved or deleted since). A lost commit response therefore never yields a second server file within the TTL. The committed key is the session row kept after the commit with `upload_sessions.committed_revision_id` set (owner decision 2026-11-13, migration `20261113100000`) until `expires_at`; it holds no reservation and does not count toward the open-session cap.

The rule matches only the node at the given parent and name (or the base node): there is no commit-by-hash across nodes. Reads: `find_live_node`, then `resolve_live_file` for the current revision id, then `get_revision` for its hash, all by indexed keys and before the session transaction; if E-13 later lets `NodeRef` or `LiveFile` carry the hash, the last read goes away. The staged path needs none of this: its receipts heal a lost response (E-14/c09).

Constants and limits: block size 2 MiB (protocol constant); `lane_limit` = `max_lanes` (4); `max_open_sessions` 8; `max_staged` 1000; `max_inflight_blocks_per_account` 32; `max_buffered_block_bytes` 1 GiB; `max_declared_size` 50 GiB; `upload_session_ttl` 24 h rolling from the last checkpoint; checkpoint every 8 MiB or 1 s.

## Invariants

- Quota is reserved before any file exists; a refusal reserves nothing and writes nothing.
- A block's bit is set only after `await_durable`; a corrupt or wrong-length body never reaches disk.
- `Overloaded` consumes 0 body bytes.
- Whoever deletes the session row (commit via 02, reaper, abort, erase) settles its reservation; never twice.
- Every lookup is account-scoped; the same `client_key`, `staged_key` in another account is independent.
- Progress is never rehydrated at start; v1.0 runs one API process.

## Non-Goals

- Not covered: commits (`commit_service.md`), downloads (`download_service.md`), HTTP headers such as `Retry-After` (11).

## Versioning Rules

- **Breaking changes** (an error removed, a cap made stricter by default, idempotency changed) require a new major version and a `contracts` change task in E-14.
- **Non-breaking additions** require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| resume-same-key | start twice with one `client_key` | same `upload_id` | IT-03-1 |
| cap-serialised | 16 concurrent starts, cap 8 | exactly 8 sessions | IT-03-1 |
| hash-before-write | flipped bit with the original hash | `HashMismatch`, bytes unchanged, bit unset | IT-03-2 |
| overloaded-untouched | 5th lane | `Overloaded`, 0 body bytes read | IT-03-2 |
| staged-zero | 0-byte staged file | `Staged{size 0}`, `block_count` 0 | IT-03-5 |
| reservation-source | random history | equals `QuotaAdmin::view.reserved_bytes` | IT-03-12 |

## Open items

- Resolved 2026-11-13 (owner): 03 takes `Arc<dyn AccountGuard>` (`PgUploadService::new(db, config, UploadPorts{guard, tree, quota, quota_admin, writer, registry, catalog}, clock)`; `catalog` added by AZ-3736, `quota_admin` (`QuotaAdmin`, for the staged quota pre-check) by AZ-3737); `PgUploadService` implements `UploadService` since AZ-3737; `put_block` opens a transaction only for a checkpoint and for the lazy load of a session this process has no entry for.
- `declared_size <= 0` on the large path (owner 2026-11-13: a zero-byte file never takes it) and a `client_key` outside 1..=256 bytes have no `UploadError` variant: 03 answers `Unexpected`; 11 must refuse both at its boundary as `invalid_request`. The staged body above 2 MiB: `LengthMismatch` (AZ-3737 proposal within the existing error list and IT-03-5; awaiting the E-14 owner, alternative `SizeTooLarge`).
- Session cap (owner 2026-11-13: every open large-path session counts): counted as open rows with `block_count > 1 OR received = '\x00'`; a staged row and a 1-block large session whose block has arrived cannot be told apart without a row-kind column, so the latter is not counted until it commits. Committed rows never count.
- Free-space floor: 04 checks it in `create_file`, after the start transaction; a refusal is compensated, so nothing stays reserved (the "floor before the transaction" order would need a 04 probe port). Floor = the smaller of `blob_free_reserve` and a share of the volume (owner 2026-11-13).
- The `BlobRegistry` port lacks the account and 04 does not implement it as a trait (AZ-4264).
- G-03-1 resolved 2026-11-11 (AZ-3731): AC-3 is 16 MiB, `max_lanes` and the checkpoint defaults stay.
- Resolved 2026-11-14 (AZ-3736): the lazy load rebuilds the write handle from `BlobCatalog::get_uploading_plan` (additive port of 04, `blob-registry-gc.md` 2.1.0).
- `upload_sessions_active` counts entries of this process's progress table: a start and a lazy load add one; every eviction (abort, commit, reap, idle flush, expiry in memory, a vanished row at checkpoint) removes one, through one `evict` helper.
- IT-03-4 measured (AZ-3736, ManualClock so only `checkpoint_bytes` triggers): case A (4 bodies cut mid-stream after 46 of 50 blocks) resends 8 MiB; case B (30 blocks acked over 4 lanes, restart before the next checkpoint) resends 4 MiB (2 blocks), within the 16 MiB bound.
- Defaults applied by AZ-3735: a same-hash duplicate of an in-flight block is `Overloaded` (retry); a repeat of an acked block is acked without reading its body (11 drains or closes it); a failing body stream is `LengthMismatch`; chunked bodies without a length are 11's 411.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
| 1.1.0 | 2026-11-11 | `NothingToSend` rule per case recorded (matched by `name_key`; folder, tombstone, other hash and no node start a session) | AZ-3732 |
| 1.2.0 | 2026-11-13 | Committed key answers `NothingToSend` with its revision until `upload_session_ttl` (owner, lost-commit recovery) | AZ-3949 |
| 1.3.0 | 2026-11-13 | `start_upload` implemented: `AccountGuard`, session-cap counting, floor after the transaction (compensated), committed-key record as `committed_revision_id` (an expired one is deleted by the next start with that key) | AZ-3734 |
| 1.4.0 | 2026-11-13 | `put_block` implemented: caps before reading, verify before write, ack after durable, checkpoint with `AccountGuard`; same-hash in-flight duplicate is `Overloaded` | AZ-3735 |
| 1.5.0 | 2026-11-14 | `get_upload`, lazy load from the checkpoint row (via `BlobCatalog::get_uploading_plan`) and `abort_upload` implemented; abort checks `AccountGuard` and leaves an expired row to the reaper | AZ-3736 |
| 1.7.0 | 2026-11-14 | Expiry rule in one place (expired row under a key reaped by the next start or stage), `SessionReaper`, `ReceiptJanitor`, `IdleFlush`, `ReservationSource`, advisory lock first on abort | AZ-3756 |
| 1.6.0 | 2026-11-14 | `stage_small` implemented (order per row above; `QuotaAdmin` port added to `UploadPorts`; body above 2 MiB `LengthMismatch` proposed; bytes without a hash `HashMismatch`); `UploadService` implemented by `PgUploadService` | AZ-3737 |
