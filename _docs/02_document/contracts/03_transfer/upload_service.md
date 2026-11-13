# Contract: upload-service (UploadService and ReservationSource behaviour)

**Component**: 03_transfer
**Producer task**: E-14/c04 — `_docs/02_tasks/todo/B03-20_transfer_start_upload.md` (contributors: E-14/c05 `put_block`, E-14/c06 `get_upload`/`abort_upload`, E-14/c07 `stage_small`, E-14/c11 `ReservationSource`; rule from spike E-14/c02); keys assigned at Step 4
**Consumer tasks**: E-14/c13, E-14/c16 (this batch); E-21/c13 (11 handlers), E-22 (doctor, `recompute` caller), E-17 (07 `recompute` caller), E-23/E-26/E-27 through the HTTP API, resolved at Step 4
**Version**: 1.3.0
**Status**: draft (the `NothingToSend` rule is recorded; frozen when the producer task passes review)
**Last Updated**: 2026-11-13

## Purpose

Getting verified bytes into a durable blob: resumable large-upload sessions over 2 MiB blocks and up to 4 lanes, and single-request staged uploads of up to 2 MiB. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 03 transfer); this file freezes the behaviour.

## Shape

Signatures: part 2 (`UploadService`, `ReservationSource`). Behaviour per method:

| Method | Behaviour | Errors |
|---|---|---|
| `start_upload` | Order: size cap, name rules, parent check, `NothingToSend` shortcut, free-space floor, then one short transaction (quota reserve, session cap, blob and session rows), file created after it, compensation on failure. Same `(account, client_key)` returns the existing session | `InvalidName`, `ParentNotFound`, `NotAFolder`, `SizeTooLarge`, `QuotaExceeded`, `SessionCapReached`, `FreeSpaceFloor`, `ClientKeyMismatch` (same key, other declared size), `AccountNotActive` |
| `get_upload` | `need_blocks`, `received_count`, `expires_at`; loads the session from its checkpoint on first access after a restart | `UploadNotFound` (also expired, foreign) |
| `put_block` | Caps checked before reading the body; length must equal `min(2 MiB, remaining)`; BLAKE3 checked before any write; ack only after durable; repeat with the same hash is idempotent | `UploadNotFound`, `IndexOutOfRange`, `LengthMismatch`, `HashMismatch`, `BlockConflict`, `Overloaded`, `StorageFull`, `Storage` |
| `abort_upload` | Deletes the session, releases exactly its `reserved_bytes`, removes the blob row; file discarded after COMMIT | `UploadNotFound` |
| `stage_small` | Key lookup first (existing row answers without writing), then cap, in-flight and quota pre-checks, then write, then one transaction (reserve, blob row, completed session row with `block_count` 0 or 1); compensation discards the file | `QuotaExceeded`, `StagedCapReached`, `HashMismatch`, `LengthMismatch`, `ClientKeyMismatch`, `FreeSpaceFloor`, `Overloaded`, `AccountNotActive`, `Storage` |
| `ReservationSource::open_reserved_bytes(account)` | Sum of `reserved_bytes` of open sessions and staged rows | none |

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

- Resolved 2026-11-13 (owner): 03 takes `Arc<dyn AccountGuard>` (`PgUploadService::new(db, config, UploadPorts{guard, tree, quota, writer, registry}, clock)`); `put_block` still opens no transaction.
- `declared_size <= 0` on the large path (owner 2026-11-13: a zero-byte file never takes it) and a `client_key` outside 1..=256 bytes have no `UploadError` variant: 03 answers `Unexpected`; 11 must refuse both at its boundary as `invalid_request`. The staged body above 2 MiB still has no name.
- Session cap (owner 2026-11-13: every open large-path session counts): counted as open rows with `block_count > 1 OR received = '\x00'`; a staged row and a 1-block large session whose block has arrived cannot be told apart without a row-kind column, so the latter is not counted until it commits. Committed rows never count.
- Free-space floor: 04 checks it in `create_file`, after the start transaction; a refusal is compensated, so nothing stays reserved (the "floor before the transaction" order would need a 04 probe port). Floor = the smaller of `blob_free_reserve` and a share of the volume (owner 2026-11-13).
- The `BlobRegistry` port lacks the account and 04 does not implement it as a trait (AZ-4264).
- G-03-1 resolved 2026-11-11 (AZ-3731): AC-3 is 16 MiB, `max_lanes` and the checkpoint defaults stay.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
| 1.1.0 | 2026-11-11 | `NothingToSend` rule per case recorded (matched by `name_key`; folder, tombstone, other hash and no node start a session) | AZ-3732 |
| 1.2.0 | 2026-11-13 | Committed key answers `NothingToSend` with its revision until `upload_session_ttl` (owner, lost-commit recovery) | AZ-3949 |
| 1.3.0 | 2026-11-13 | `start_upload` implemented: `AccountGuard`, session-cap counting, floor after the transaction (compensated), committed-key record as `committed_revision_id` (an expired one is deleted by the next start with that key) | AZ-3734 |
