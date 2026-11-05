# Contract: blob-reader

**Component**: 04_blobstore
**Producer task**: E-10/c08 — `_docs/02_tasks/todo/B02-08_blob_reader.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-14 transfer (downloads with Range, `read_head` sniff at commit), E-18 media_jobs (internal worker input fetch, derived opener), E-10/c12 scrub (shares the verification rules), E-22 pipelines (download, media stage 1, restore)
**Version**: 1.0.0
**Status**: draft (frozen when E-10/c08 is accepted)
**Last Updated**: 2026-11-04

## Purpose

How `BlobReader` returns plaintext and how it fails. There is one reader and one decrypt path for user downloads and for the internal worker fetch (AC-72). The trait signature and `BlobError` live in `contracts` (E-00/c05, contracts part 3). Source: `components/04_blobstore/description.md` §3, §4, §8.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `open` | `blob_id, wrapped_key, master_key_id, plaintext_size, range: Option<ByteRange>` -> `PlaintextStream` | `BlobMissing`, `RangeNotSatisfiable`, `Corrupt`, `KeyUnavailable` | async |
| `read_head` | same as `open` without range, `max_bytes <= 64 KiB` -> `Bytes` | as `open` | async |

The caller passes the key material from `BlobCatalog::get_ref` (see `blob-registry-gc.md`); the reader does not read `blobs` per request and keeps no key cache.

## Invariants

- The first chunk of the requested range is read and authenticated before `open` returns. Later chunks are read as the stream is polled, with read-ahead bounded by `blob_read_ahead_chunks` (default 16 chunks).
- A chunk whose tag fails yields `Err(Corrupt)` and the stream ends. The stream never yields a complete-looking short body: a consumer always sees the error.
- Tampering of any kind (bit flip, truncation, appended bytes, swapped or foreign chunk, changed header, dropped final chunk, another blob's wrapped key) yields `Corrupt` or `KeyUnavailable`, never plaintext.
- A 4 KiB range reads at most 2 chunks. A range with `start == size`, `end > size` or `start > end` is `RangeNotSatisfiable`.
- A missing file is `BlobMissing`: typed, never `Io`, never a panic. Callers map it to `content_unavailable` (404), never to a 5xx (AC-73).
- A master key id that the loaded ring does not hold is `KeyUnavailable`; no byte is returned.
- The first `Corrupt` or `BlobMissing` for a blob sets `blobs.unavailable_at` and writes one `scrub_mismatch` audit event per blob per process run.
- A 1 GiB read without range returns bytes whose BLAKE3 equals the plaintext; peak memory growth stays at or below 32 MiB.
- Error text and logs carry blob and account ids only.

## Non-Goals

- Not covered: Range header parsing, ETag, disposition, media type (03, 11); account scoping (done by `BlobCatalog::get_ref` and the caller).
- Not covered: how a clean re-read clears `unavailable_at` (open point, flagged in E-10/c08).

## Versioning Rules

- **Breaking changes** (a corrupt chunk may yield bytes, an error changes meaning, the single-reader rule is dropped) require a new major version and notice to E-14 and E-18.
- **Non-breaking additions** (new optional input, a new error variant callers already treat as unexpected) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| round-trip | blob of 5 MiB + 7 written in shuffled block order | bytes and BLAKE3 equal the source | IT-04-1 |
| range-4k | 4096 bytes at offset 65,534 of a 3 MiB blob | bytes equal the source slice; 2 chunk reads | IT-04-3 |
| range-out-of-bounds | `start == size` | `RangeNotSatisfiable` | IT-04-3 |
| tamper-later-chunk | bit flipped in chunk 10, full read | early chunks returned, then `Err(Corrupt)`, stream ends | IT-04-4 |
| missing-file | file deleted | `Err(BlobMissing)`; `unavailable_at` set | IT-04-15 |
| foreign-wrap | B1's `wrapped_key` used for B2 | `Corrupt` or `KeyUnavailable`, 0 bytes | ST-04-3 |
| one-gib | DS-25, no range, 1 MiB reads | BLAKE3 equal; RSS growth <= 32 MiB | IT-04-18 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
