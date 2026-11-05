# Contract: upload-service (UploadService and ReservationSource behaviour)

**Component**: 03_transfer
**Producer task**: E-14/c04 — `_docs/02_tasks/todo/B03-20_transfer_start_upload.md` (contributors: E-14/c05 `put_block`, E-14/c06 `get_upload`/`abort_upload`, E-14/c07 `stage_small`, E-14/c11 `ReservationSource`; rule from spike E-14/c02); keys assigned at Step 4
**Consumer tasks**: E-14/c13, E-14/c16 (this batch); E-21/c13 (11 handlers), E-22 (doctor, `recompute` caller), E-17 (07 `recompute` caller), E-23/E-26/E-27 through the HTTP API, resolved at Step 4
**Version**: 1.0.0
**Status**: draft (frozen when the producer task passes review and E-14/c02 has recorded the `NothingToSend` rule)
**Last Updated**: 2026-11-04

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

- `NothingToSend` rule per case (E-14/c02).
- `AccountNotActive` at start, put and stage needs the `AccountGuard` port, which 03's Ports table does not list; `put_block` opens no transaction.
- Error names for `declared_size = 0` on the large path and for a staged body above 2 MiB.
- `data_model.md` §6.3 counts open sessions as rows with `block_count > 1`, missing large sessions of at most 2 MiB.
- G-03-1 may change `max_lanes` or checkpoint defaults (E-14/c01).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
