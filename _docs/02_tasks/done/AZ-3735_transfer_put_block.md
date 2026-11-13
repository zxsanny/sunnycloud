# UploadService put_block: verify before write, caps, durability, checkpoints

**Task**: AZ-3735_transfer_put_block
**Name**: Transfer put_block
**Description**: Receive one 2 MiB block: check in-flight caps before reading, verify length and BLAKE3 before any write, write and wait for the group fdatasync, then set the bit, and checkpoint progress every 8 MiB or 1 s
**Complexity**: 5 points
**Dependencies**: AZ-3733_transfer_skeleton, AZ-3734_transfer_start_upload, AZ-3766_sc_ids, AZ-3726_blob_writer, AZ-3727_group_commit_barrier
**Component**: 03_transfer
**Tracker**: AZ-3735
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — this task adds `put_block`.

## Problem

A block that is acknowledged must survive a power cut (AC-38), a corrupt or wrong-length body must never reach disk (AC-7; also avoids nonce reuse in 04), repeats over flaky lanes must be harmless, and one user must not exhaust the server's buffers (R13). The ack rate (about 56 blocks/s for 100 sessions) rules out a database write per block, so progress lives in memory with periodic checkpoints.

## Outcome

- `put_block` answers `BlockAck` only after the block is durable; before that the bit is unset.
- Caps are checked before any body byte is read; hash and length are checked before any byte is written.
- A repeat with the same hash is idempotent; other bytes for an acked or in-flight index are `BlockConflict`.

## Scope

### Included
- In-flight caps: session in-flight bits against `lane_limit`, account total in-flight blocks (large and staged) against `max_inflight_blocks_per_account`, the global `max_buffered_block_bytes` semaphore; any excess is `Overloaded` with 0 body bytes consumed.
- Length rule: body must equal `min(2 MiB, remaining)`; shorter, longer or chunked without length is `LengthMismatch`; `IndexOutOfRange`; `UploadNotFound`.
- Buffer, BLAKE3 against `expected_hash` (`HashMismatch`), then `BlobWriter::write_block` and `await_durable`; `Storage` on I/O error and `StorageFull` from 04.
- Set the received bit and `block_hashes[index]` in the progress table, checkpoint `received` and `block_hashes` to the session row every `checkpoint_bytes` or `checkpoint_interval`, rolling `expires_at`.
- Metrics `blocks_received_total`, `block_bytes_received_total`; WARN on hash mismatch; no per-block log lines.

### Excluded
- Lazy load after restart and `get_upload` (E-14/c06); idle flush (E-14/c11).

## Acceptance Criteria

**AC-1: Correct block acked after durable (AC-38)**
Given a 9 MiB session (4 full blocks and a 1 MiB tail)
When each block is sent with its correct hash and length
Then each returns `BlockAck`, and the tail block's accepted length is 1 MiB

**AC-2: Bad hash or length never written (AC-7)**
Given the same session
When a block with one flipped bit and the original hash, a body 1 byte short, 1 byte long, chunked without length, and indexes 5 and −1 are sent
Then they return `HashMismatch` (file bytes at that offset unchanged and bit unset), `LengthMismatch` x3 and `IndexOutOfRange` x2

**AC-3: Duplicates and conflicts**
Given block 0 acked
When block 0 is sent again with the same bytes, and then with other bytes
Then the first returns `BlockAck` with no second write, and the second returns `BlockConflict` with the file unchanged

**AC-4: Caps refuse before reading (R13)**
Given 4 lanes busy on one session, an account with 32 blocks in flight, and the global buffer exhausted, in separate runs
When one more block is sent in each run
Then each returns `Overloaded` and 0 body bytes were consumed

**AC-5: Ack waits for fdatasync (AC-38)**
Given a decorator that delays `await_durable` by 300 ms, and then one that returns an I/O error
When a block is sent in each case
Then the first stays unresolved for at least 300 ms with the bit unset until release, and the second returns `Storage`, leaves the bit unset, keeps the index in `need_blocks`, and a resend of the same bytes succeeds

**AC-6: Lane races assemble the right file (AC-7)**
Given DS-34 slices from 4 MiB + 1 B to 48 MiB
When 4 tasks send blocks in random order with 10% duplicates (fixed seed)
Then the bitmap is complete, `block_hashes` equal the manifest, the whole-file hash equals the manifest hash, and 0 `BlockConflict` occurs for identical duplicates

**AC-7: Checkpoints**
Given `checkpoint_bytes = 8 MiB` and `checkpoint_interval = 1 s`
When 5 blocks are acked quickly, and then one block after a 1 s pause
Then the session row's `received` and `block_hashes` are rewritten after the 4th block (8 MiB) and after the interval, and `expires_at` moves forward with each checkpoint

## Non-Functional Requirements

**Performance**
- Single-session ingest at least 250 MB/s (fail 112 MB/s) and ack p95 at most 150 ms (fail 400 ms) for 100 sessions x 4 lanes; measured by PT-03-1 in E-14/c14.

**Reliability**
- The bit is set only after `await_durable` returns.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Expected length per index | `min(2 MiB, remaining)` |
| AC-3 | Duplicate-block rules | Same hash idempotent; other bytes conflict |
| AC-4 | Cap accounting with staged writes | Staged writes count against the account cap |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 9 MiB session | Correct blocks (IT-03-2) | `BlockAck`; tail 1 MiB | FT-P-06 |
| AC-2 | Same | Bad hash, length, index (IT-03-2) | Typed errors; nothing written | FT-N-01, NFT-SEC-20, NFT-SEC-21 |
| AC-3 | Block 0 acked | Same and other bytes (IT-03-2) | Idempotent; `BlockConflict` | NFT-SEC-20 |
| AC-4 | Lane, account, global caps | Extra block (IT-03-2) | `Overloaded`, body untouched | NFT-SEC-15 |
| AC-5 | Delay and error decorators | Send (IT-03-3) | Ack after durable; `Storage` path | NFT-RES-03 |
| AC-6 | DS-34 slices | Random 4-lane order (IT-03-20) | Complete, hashes match | |
| AC-7 | ms-scale checkpoint config | Ack pattern | Checkpoints at 8 MiB and interval | AC-3 |

## Constraints

- Real 04 writer and group-commit barrier on a temp directory; delays and errors come from a thin decorator over the real port.

## Risks & Mitigation

**Risk 1: Buffer pressure under many sessions**
- *Risk*: 100 sessions x 4 lanes x 2 MiB buffers exceed memory.
- *Mitigation*: global byte semaphore and per-account cap; `Overloaded` with `Retry-After` in 11.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `put_block` must refuse a deleting account (03 §7, IT-03-15) but its error list has no `AccountNotActive` and it opens no transaction; how the check is made is not stated | E-14 owner | resolved 2026-11-13 (owner): checked through `AccountGuard` in the checkpoint transaction (every 8 MiB or 1 s), no extra call per block; a deleting account is refused within one checkpoint interval (its credentials are revoked at the deletion request anyway) | Medium |
| ST-04-8: 04's rule for a rewrite with different bytes is open (E-10/c01); 03 refuses with `BlockConflict` before calling 04 | Owner via E-10 | resolved 2026-11-12 (batch 48): identical bytes accepted, different bytes `BlockConflict` (04 `write_block_once`) | Low |
