# BlobReader: random access, read-ahead, read_head and tamper handling

**Task**: AZ-3728_blob_reader
**Name**: Blob reader
**Description**: The single `BlobReader` that streams verified plaintext for whole files and byte ranges, reads at most two chunks for a 4 KiB range, never returns tampered bytes, and marks blobs it finds corrupt or missing
**Complexity**: 5 points
**Dependencies**: AZ-3726_blob_writer, AZ-3729_blob_registry_gc_queue, AZ-3784_sc_audit
**Component**: 04 blobstore
**Tracker**: AZ-3728
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c08

## Problem

Downloads (03), share links, media input fetch by the worker (08) and the commit-time type sniff all read encrypted blobs. A 4 KiB range must take at most 5 ms at p95 (AC-43), a 1 GiB worker fetch must equal the plaintext (AC-72), and corruption must never be served as a good file (AC-7). A missing file after a restore must be a typed result, never a server error (AC-73).

## Outcome

- `open` takes the caller's key material and an optional range, authenticates the first chunk before it returns, then streams later chunks with read-ahead of `blob_read_ahead_chunks`.
- A failed tag yields `Err(Corrupt)` and ends the stream; a missing file is `BlobMissing`; an unknown key id is `KeyUnavailable`; out-of-bounds ranges are `RangeNotSatisfiable`.
- `read_head` returns up to 64 KiB from the start for type sniffing.
- The first `Corrupt` or `BlobMissing` for a blob sets `blobs.unavailable_at` and writes one `scrub_mismatch` audit event per blob per process run.
- This reader is the only decrypt path in the crate.

## Scope

### Included
- `open`, `read_head`, range-to-chunk use, read-ahead, unavailable marking and the audit event on read.

### Excluded
- Range header parsing and HTTP headers (03, 11); account scoping (`BlobCatalog`, E-10/c09); scrub (E-10/c12); read metrics (E-10/c15).

## Acceptance Criteria

**AC-1: Round trip**
Given blobs of 0, 1, 65,535, 65,536, 65,537 bytes, 2 MiB and 5 MiB + 7 written block by block in shuffled order, registered and committed
When each is opened without a range
Then the bytes and their BLAKE3 equal the source

**AC-2: Ranges**
Given a 3 MiB blob
When the first byte, the last byte, 4096 bytes at offsets 0, 65,534 and 1,000,003, a range spanning 3 chunks and the whole file are read, and `read_head(64 KiB)` is called
Then every result equals the source slice, each 4 KiB range reads at most 2 chunks (2 x 65,552 bytes recorded by the file decorator), and `read_head` returns the first 64 KiB

**AC-3: Unsatisfiable input**
Given the same blob
When ranges with `start == size`, `end > size` or `start > end`, or `read_head` with more than 64 KiB, are requested
Then the ranges return `RangeNotSatisfiable` and the oversized `read_head` is rejected with a typed error, with no bytes returned

**AC-4: Tampered files never yield plaintext**
Given copies of a blob with a flipped bit in chunk k, truncation at and inside a chunk, 16 appended bytes, two chunks swapped, a chunk copied from another blob, a changed header nonce prefix, and the final-flag chunk dropped
When ranges avoiding and touching the damage, and full reads, are consumed
Then ranges avoiding the damage are correct; any read touching it ends with `Err(Corrupt)` (at `open` for the first chunk, later in the stream otherwise) and the consumer never sees a complete-looking short body

**AC-5: Findings are recorded once**
Given the first `Corrupt` for a blob in a process run, and a second one for the same blob
When the reads finish
Then `blobs.unavailable_at` is set and exactly one `scrub_mismatch` audit event exists for that blob

**AC-6: Missing file and unknown key are typed**
Given a committed blob whose file was deleted, and a blob whose master key id the ring lacks
When `open` and `read_head` are called
Then the first returns `BlobMissing` (not `Io`, no panic) and sets `unavailable_at`, and the second returns `KeyUnavailable`; neither returns a byte

**AC-7: 1 GiB equals the plaintext with bounded memory**
Given DS-25 (1 GiB) stored as a blob
When it is opened without a range and consumed in 1 MiB reads
Then the BLAKE3 of the stream equals the generator manifest hash and peak RSS growth stays at or below 32 MiB (global AC-72, single-reader part)

**AC-8: Reopened blob reads back**
Given a blob continued with `open_for_write` after a dropped handle
When it is read
Then the bytes equal the source

## Non-Functional Requirements

**Performance**
- 4 KiB range p95 at most 2 ms warm and 4 ms cold, failure 5 ms (PT-04-1); single-stream read at least 500 MB/s, failure 112 MB/s (PT-04-2). Measured in E-10/c14.

**Reliability**
- No code path returns bytes after a failed tag.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Read-ahead window size | Never more than `blob_read_ahead_chunks` chunks buffered |
| AC-4 | Stream state after `Corrupt` | Stream ends; no further item |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Chunk-size corpus | IT-04-1 round-trip part | Byte and BLAKE3 equal | AC-43 |
| AC-2 | 3 MiB blob, file decorator | IT-04-3 | Slices equal; <= 2 chunk reads per 4 KiB | PT-04-1 |
| AC-3 | 3 MiB blob | IT-04-3 out-of-bounds cases | `RangeNotSatisfiable`; oversized head rejected | |
| AC-4 | Damage cases (a)-(h) | IT-04-4 | `Corrupt`; never a short body | FT-N-02, NFT-SEC-25 |
| AC-5 | Damaged blob read twice | IT-04-4 unavailable and audit check | One audit event; `unavailable_at` set | |
| AC-6 | Deleted file; ring without id | IT-04-15 open part; ST-04-2 open case | `BlobMissing`; `KeyUnavailable` | NFT-RES-37 |
| AC-7 | DS-25 | IT-04-18 | BLAKE3 equal; RSS growth <= 32 MiB | FT-N-43 |
| AC-8 | Reopened blob | IT-04-16 read part | Bytes equal | |

## Constraints

- Stateless: the caller passes the key material from `BlobCatalog::get_ref`; no key cache, one unwrap per open.
- No database transaction is held across file reads; the `unavailable_at` update and the audit row use their own short transaction.
- Logs carry ids only; no per-chunk log lines.

## Risks & Mitigation

**Risk 1: Short body taken as a whole file**
- *Risk*: a consumer stops at a clean chunk boundary and treats the body as complete.
- *Mitigation*: the stream ends with an error item, never with a normal end, after a failed tag (AC-4).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The description says a clean re-read clears `unavailable_at`, but the reader is stateless and does not read `blobs` per request; how and when the clear happens is not stated | Epic owner | open | Low |
| No error variant is named for `read_head` with `max_bytes` above 64 KiB (IT-04-3 says "rejected") | Epic owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/04_blobstore/blob-reader.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
