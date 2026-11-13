# Contract: blob-writer

**Component**: 04_blobstore
**Producer task**: E-10/c06 — `_docs/02_tasks/todo/B02-06_blob_writer.md`; the `await_durable` barrier part: E-10/c07 — `B02-07_group_commit_barrier.md` (both renamed to their AZ keys at Step 4)
**Consumer tasks**: E-14 transfer (start_upload, put_block, stage_small, session reaper), E-18 media_jobs (derived outputs through `write_complete`), E-22 pipelines (large and small upload, media)
**Version**: 1.1.0
**Status**: draft (frozen when E-10/c06 is accepted)
**Last Updated**: 2026-11-11

## Purpose

The behaviour every caller of `BlobWriter` relies on: when bytes are durable, when a row may exist, and which typed failure means what. The trait signature and the `BlobError` enum are declared in the `contracts` crate (E-00/c05, contracts part 3); this file fixes the semantics on top of that signature. Source: `components/04_blobstore/description.md` §3, §5.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `plan` | `account_id, plaintext_size` -> `BlobPlan{blob_id, wrapped_key, master_key_id}` | `KeyUnavailable` | sync, pure |
| `create_file` | `BlobPlan, preallocate: bool` -> `BlobHandle` | `NoSpace`, `FreeSpaceFloor`, `Io` | async |
| `open_for_write` | `blob_id, wrapped_key, master_key_id, plaintext_size` -> `BlobHandle` | `BlobMissing`, `KeyUnavailable` | async |
| `write_block` | `BlobHandle, block_index, plaintext block (hash verified by the caller)` -> `DurableToken` | `NoSpace`, `Io`, `OutOfRange`, `BlockConflict` | async |
| `await_durable` | `DurableToken` -> `()` | `Io` | async |
| `write_complete` | `account_id, size, ByteStream` -> `WrittenBlob{plan, size}` | `NoSpace`, `FreeSpaceFloor`, `SizeMismatch`, `Io` | async |
| `discard` | `blob_id` -> `()` | `Io` | async |

`ByteStream` is a neutral stream of byte chunks with a stream error; no framework request type crosses the port.

## Invariants

- `plan` does no I/O: it picks a new v4 blob id and wraps a fresh data key under the current master key id. No file and no row exist after `plan`.
- `create_file` refuses with `FreeSpaceFloor` when the volume's free space would fall below the free-space floor = the smaller of `blob_free_reserve` (10 GiB) and `blob_free_reserve_percent` (5) of the volume's size (owner decision 2026-11-13); it then leaves no file. With `preallocate = true` the whole ciphertext length is reserved, so a later `write_block` of that blob does not fail with `NoSpace` because other writers filled the disk. The directory entry is synced once at create.
- `write_block` encrypts the block and writes it at the offset fixed by the file format; blocks may arrive in any order and from several tasks. A returned `DurableToken` does **not** mean durable.
- `await_durable` returns `Ok` only after an `fdatasync` that started after the write has completed. `Err(Io)` means the caller must not acknowledge the block; every waiter of a failed group gets `Io`. There is no artificial wait: one uncontended write is acknowledged in at most 20 ms.
- `write_complete` writes the whole object and syncs it before it returns, and creates **no** `blobs` row; the caller registers the row afterwards (a crash in between leaves an orphan file that the orphan sweep removes). Any error leaves no partial file. A stream shorter or longer than `size` is `SizeMismatch`.
- Upload sessions write the row (state `uploading`) before the file; `write_complete` callers write the file before the row (AC-68).
- `discard` of a missing file is `Ok`.
- Writing one block index twice (decided 2026-11-11, AZ-3721): a second write of an index that already holds a block succeeds only with identical bytes (the stored block is decrypted and compared; the stored ciphertext is written back unchanged, never re-sealed, and the same `DurableToken` semantics apply, owner decision 2026-11-12; a never-written all-zero region is a first write; a torn region that does not authenticate is `BlockConflict`); different bytes fail with `BlockConflict` and nothing is sealed or written, so no two plaintexts ever share a key and nonce. A caller that gets `BlockConflict` cannot finish that blob: 03 maps it to its own `BlockConflict` and the client restarts with a new session. A re-send of the same bytes after a crash (the checkpoint lagged the disk) is therefore always accepted. 03's caller-side rule (hash verified before `write_block`, ST-03-2) stays and is sufficient.
- The writer never holds a database transaction across a sync or any file I/O. Error text carries ids only; key material never appears in errors, logs or metrics.

## Non-Goals

- Not covered: `blobs` row writes (`BlobRegistry`, see `blob-registry-gc.md`), quota, upload-session bitmaps and checkpoints (03), hash verification (03).
- Not covered: reading (`blob-reader.md`).

## Versioning Rules

- **Breaking changes** (a method's error meaning changes, durability guarantee weakened, row-creation rule changed) require a new major version and notice to E-14 and E-18.
- **Non-breaking additions** (new optional field, a new error variant callers already treat as unexpected) require a minor version bump.
- The block-rewrite rule from E-10/c01 is added as a minor version (it narrows undefined behaviour).

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| plan-pure | `plan(account, 5 MiB)` | new v4 id, wrapped key, current key id; no file, no row | |
| floor-refusal | 64 MiB volume, reserve 8 MiB, `create_file` for 60 MiB | `FreeSpaceFloor`, no file | IT-04-7 |
| preallocated-full-disk | 20 MiB preallocated blob, filler fills the rest, write every block | every `write_block` and `await_durable` succeed | IT-04-7 |
| ack-after-sync | sync blocked 300 ms | `await_durable` pending >= 300 ms | IT-04-5 (a) |
| sync-failure | sync fails for one group | every waiter gets `Io`; next group succeeds | IT-04-5 (b) |
| write-complete-no-row | `write_complete(1 B)` | `WrittenBlob`; `blobs` has no row for the id | IT-04-6 |
| size-mismatch | stream 1 byte short | `SizeMismatch`, no file left | IT-04-6 |
| discard-missing | `discard(unknown id)` | `Ok` | IT-04-6 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
| 1.1.0 | 2026-11-11 | Block-rewrite rule: identical bytes accepted, different bytes `BlockConflict`; new `BlobError::BlockConflict` variant for E-10/c06 in `contracts` part 3 | AZ-3721 |
