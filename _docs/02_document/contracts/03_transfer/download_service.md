# Contract: download-service (DownloadService behaviour)

**Component**: 03_transfer
**Producer task**: E-14/c10 — `_docs/02_tasks/todo/B03-26_transfer_download.md`; key assigned at Step 4
**Consumer tasks**: E-14/c13, E-14/c14, E-14/c16 (this batch); E-21/c10, E-21/c13, E-21/c14 (11 content, share and derived routes), resolved at Step 4
**Version**: 1.0.1
**Status**: draft (frozen when the producer task passes review)
**Last Updated**: 2026-11-04

## Purpose

Streaming stored content back with Range and validators, and deciding inline versus attachment, for user downloads and for share and derived downloads that 11 composes with 06 or 08. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 03 transfer).

## Shape

Signatures: part 2 (`DownloadService`, `Download`). Behaviour:

| Method | Behaviour | Errors |
|---|---|---|
| `open_revision_content(account, Revision \| Current(node), retained_ok, range?, if_range?, if_none_match?)` | `Current(node)` through `TreeReader::resolve_live_file` (folder or gone is `NotFound`); `Revision` through `get_revision`, `NotFound` when not current unless `retained_ok` | `NotFound`, `RangeNotSatisfiable`, `ContentUnavailable`, `Storage` |
| `open_blob_content(account, BlobRef, media_type?, same options)` | Same serving rules for a blob reference supplied by 11 from 06 or 08; account-scoped | same |

| Rule | Result |
|---|---|
| No range | `Full`, `total_size`, ETag = blob id (strong) |
| One satisfiable range | `Partial` with exactly those bytes |
| Multi-range or overlapping ranges | ignored: `Full` |
| Out of bounds | `RangeNotSatisfiable { total_size }` (the size of the whole content, for `Content-Range: bytes */<total_size>`) |
| `If-Range` with a stale ETag | `Full` |
| `If-None-Match` equal to the ETag | `NotModified` |
| HEAD | same call, body dropped by 11 |
| Sniffed type image/jpeg, png, gif, webp, avif, video/mp4, webm | `Inline` |
| Any other type or no type | `Attachment` (file name never consulted) |

## Invariants

- Users never receive a non-current revision; only an admin principal (11 sets `retained_ok`) may.
- The first chunk is verified before the result is returned; a later `Corrupt` ends the stream with an error, never a complete-looking short body (AC-7).
- A missing blob file is `ContentUnavailable` (404 with its own code), never a server-class error.
- A foreign revision, node or blob is `NotFound` (AC-59).
- Memory per stream is bounded by 04's read-ahead; no multipart responses.

## Non-Goals

- Not covered: HTTP headers (`nosniff`, sandbox CSP, `Accept-Ranges`) and status codes (E-21/c10); share token checks (06); derived lookups (08).

## Versioning Rules

- **Breaking changes** (allowlist widened, retained rule relaxed, multi-range support) require a new major version and a `contracts` change task in E-14.
- **Non-breaking additions** require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| single-range | `bytes=0-0` on 5 MiB | `Partial`, 1 byte | IT-03-13 |
| multi-range | `bytes=0-1,5-9` | `Full` | IT-03-13 |
| retained-user | old revision, `retained_ok = false` | `NotFound` | IT-03-13 |
| hostile-inline | HTML bytes named `a.jpg` | `Attachment` | ST-03-4 |
| corrupt-chunk | flipped byte in chunk 40 | error after chunk 39, never a short complete body | IT-03-14 |

## Open items

- Source of `suggested_name` for `open_blob_content` (its `BlobRef` carries no name).
- G-10 is still an owner item in `tests/spec-gaps.md`, while 03 decides inline by sniffed type.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
