# BlobWriter: plan, create, block writes, write_complete, discard

**Task**: AZ-3726_blob_writer
**Name**: Blob writer
**Description**: The `BlobWriter` port: pure planning, file creation with preallocation and the free-space floor, encrypted out-of-order block writes acknowledged through the group-commit barrier, whole-object writes for small and derived blobs, and discard
**Complexity**: 5 points
**Dependencies**: AZ-3724_blob_file_format, AZ-3725_master_keyring, AZ-3727_group_commit_barrier; notes: E-31/c06 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 04 blobstore
**Tracker**: AZ-3726
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c06

## Problem

Uploads (03) write 2 MiB blocks over up to four lanes in any order and may acknowledge a block only when it is on disk (AC-38). Small files and derived outputs (03 staged path, 08) are written whole. A running upload must not fail because other writers filled the disk, and the disk must never drop below the configured floor (AC-39). Callers need one port with clear rules about durability and about when a row may exist (AC-68).

## Outcome

- `plan` returns a new v4 blob id and a data key wrapped under the current master key id, with no I/O.
- `create_file` writes the header, reserves the whole ciphertext length when `blob_preallocate` is on, syncs the directory entry once, and refuses below `blob_free_reserve` with `FreeSpaceFloor`.
- `open_for_write` reopens a partly written blob from its row's key material after a restart or from another request.
- `write_block` encrypts the block's chunks and writes them at their fixed offset; the returned `DurableToken` becomes durable through `await_durable` (barrier from E-10/c07).
- `write_complete` writes and syncs a whole object and returns `WrittenBlob` without creating a row; `discard` unlinks, and a missing file is `Ok`.

## Scope

### Included
- All `BlobWriter` methods, free-space check against the volume, preallocation, the blocking I/O pool sized by `blob_io_threads`.

### Excluded
- `blobs` row writes (E-10/c09), the barrier internals (E-10/c07), the free-space metrics and health probe (E-10/c13, c15), the block-rewrite rule (E-10/c01).

## Acceptance Criteria

**AC-1: Planning is pure**
Given an account and a declared size
When `plan` runs
Then it returns a v4 blob id, a wrapped key and the current master key id, and afterwards no file and no `blobs` row exist for that id

**AC-2: Out-of-order writes give the specified layout**
Given declared sizes 0, 1, 65,535, 65,536, 65,537, 2 MiB and 5 MiB + 7 with the DS-09 plaintext marker every 1 MiB
When blocks are written in shuffled order from 4 tasks and each is awaited durable
Then each file length is 64 + n_chunks * 16 + size, the header magic and format version are present, a search for the marker over the whole file finds 0 hits, and two blobs with identical plaintext differ in ciphertext and wrapped key

**AC-3: Floor refusal and preallocation**
Given a 64 MiB loop volume and `blob_free_reserve = 8 MiB`
When a 60 MiB file is created, and then a 20 MiB preallocated blob is created, a filler file takes all other space and every block of the blob is written
Then the 60 MiB create returns `FreeSpaceFloor` and leaves no file, every block write of the preallocated blob succeeds, and the directory entry is synced exactly once per created file

**AC-4: write_complete writes the file and no row**
Given streams of 0 B, 1 B and 3 MiB, a stream 1 byte short and 1 byte long, a stream that errors mid-way, a full volume and a volume below the floor
When `write_complete` runs for each
Then the three good streams return `WrittenBlob{plan, size}` with the file synced and no `blobs` row created, and the others return `SizeMismatch`, `SizeMismatch`, `Io`, `NoSpace` and `FreeSpaceFloor`, each leaving no partial file

**AC-5: discard**
Given an existing blob file and an unknown id
When `discard` runs for each
Then the file is unlinked and the unknown id returns `Ok`

**AC-6: Reopen after a dropped handle**
Given a blob with half its blocks written and its handle dropped
When `open_for_write` is called with the row's key material, with an unknown master key id, and for a deleted file
Then the first continues writing, the second returns `KeyUnavailable` and the third `BlobMissing`

**AC-7: Block bounds**
Given a blob of declared size 5 MiB
When `write_block` is called with an index beyond the declared size
Then it returns `OutOfRange` and writes nothing

**AC-8: Acknowledgement waits for the sync**
Given the barrier's sync function blocked for 300 ms
When a block is written and its token awaited
Then `await_durable` does not return before 300 ms

## Non-Functional Requirements

**Performance**
- Single-stream write with encryption at least 300 MB/s, failure below 112 MB/s (PT-04-2, measured in E-10/c14).

**Reliability**
- A preallocated upload never fails with `NoSpace` because of other writers (AC-3); free disk never falls below the floor through `create_file` or `write_complete`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `plan` performs no I/O | No file, no row |
| AC-7 | Index bounds for the last block | `OutOfRange` beyond the size |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Chunk-size corpus, DS-09 marker | IT-04-1 layout and no-plaintext part (round trip read in E-10/c08) | Layout formula, 0 marker hits, distinct ciphertext | AC-43 |
| AC-3 | 64 MiB loop volume, 8 MiB reserve | IT-04-7 create part; IT-04-5 directory-sync counter | `FreeSpaceFloor`; preallocated writes succeed; 1 directory sync | NFT-RES-LIM-01 |
| AC-4 | Stream variants, small loop volume | IT-04-6 | `WrittenBlob` or typed error; no row, no partial file | NFT-RES-04 |
| AC-5 | Existing and unknown id | IT-04-6 discard case | Unlinked; `Ok` | |
| AC-6 | Half-written blob | IT-04-16 `open_for_write` part | Continues; `KeyUnavailable`; `BlobMissing` | |
| AC-8 | Sync blocked 300 ms | IT-04-5 (a) through `write_block` | Pending >= 300 ms | AC-38 |

## Constraints

- No database transaction is opened by the writer. Logs carry blob and account ids only; no per-block log lines.
- Loop-volume rows need the blob volume tool of E-31/c06 and a privileged runner; without them they report `NOT-RUN(C-priv)` and the task is not done.
- Until E-10/c01 decides the rewrite rule, the behaviour of a second `write_block` for the same index is not defined and is not tested.

## Risks & Mitigation

**Risk 1: Partial file after a failure**
- *Risk*: a failed `write_complete` leaves bytes that look like a blob.
- *Mitigation*: every failure path removes the file (AC-4); anything left by a crash has no row and is swept by E-10/c11.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Block-rewrite rule (ST-04-8) pending owner decision in E-10/c01 | Owner | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/04_blobstore/blob-writer.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
