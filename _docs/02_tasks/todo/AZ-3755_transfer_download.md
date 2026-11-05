# DownloadService: Range, validators, serving decision, retained rules, corruption

**Task**: AZ-3755_transfer_download
**Name**: Transfer DownloadService
**Description**: Stream a revision or blob back with one byte range, strong ETag, conditional requests, the inline-or-attachment decision and the rule that users never get non-current revisions; fail cleanly on corruption or a missing blob
**Complexity**: 5 points
**Dependencies**: AZ-3733_transfer_skeleton, AZ-3738_transfer_commit_upload, AZ-3707_tree_reader, AZ-3716_tree_revision_lookups, AZ-3728_blob_reader, AZ-3729_blob_registry_gc_queue
**Component**: 03_transfer
**Tracker**: AZ-3755
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/03_transfer/download_service.md` — produced by this task.
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `resolve_live_file`, `get_revision`.

## Problem

Downloads must stream 50 GiB with bounded memory (AC-51), serve a 4 KiB range fast (AC-43), never render hostile content inline (AC-47), never hand users an old revision that is kept only for operator restore, and never return a short body that looks complete when a stored chunk is corrupt (AC-7). Shares and derived files reuse the same service with a blob reference instead of a revision (11 composes 06 or 08 with it).

## Outcome

- `open_revision_content` and `open_blob_content` return `Download{status Full|Partial|NotModified, total_size, range, etag, media_type, disposition, suggested_name, body}` or `NotFound`, `RangeNotSatisfiable`, `ContentUnavailable`, `Storage`.
- Exactly one range is honoured; ETag is the blob id; `Inline` only for the 7 allowlisted sniffed types.
- A corrupt chunk ends the stream with an error; a missing blob is `ContentUnavailable`, never a server error.

## Scope

### Included
- `NodeRef(current)` through `TreeReader::resolve_live_file` (folder or `Gone` is `NotFound`); `RevisionRef` through `get_revision`, `NotFound` for a non-current revision unless `retained_ok`.
- Range: one range honoured; multi-range or overlapping ignored (Full); out of bounds `RangeNotSatisfiable`; `If-Range` with a stale ETag gives Full; `If-None-Match` equal gives `NotModified`; HEAD uses the same call without the body.
- Disposition `Inline` only for image/jpeg, png, gif, webp, avif and video/mp4, webm by sniffed type; everything else (and no type) `Attachment`; the file name is never consulted.
- Body from 04's reader: first chunk verified before the result is produced; a later `Corrupt` ends the stream with an error; a missing file is `ContentUnavailable`.
- Account scoping on every lookup; metric `download_bytes_total`.

### Excluded
- HTTP headers (`nosniff`, CSP, `Accept-Ranges`) and status mapping (E-21/c10); share token checks (E-16); derived lookups (E-18).

## Acceptance Criteria

**AC-1: Ranges and validators (AC-43, AC-51)**
Given a 5 MiB file
When it is read in full and with `bytes=0-0`, `bytes=4194304-`, `bytes=-4096`, `bytes=0-1,5-9`, an overlapping multi-range, `bytes=9999999-`, `If-Range` with a stale ETag and `If-None-Match` with the current ETag
Then the results are Full with `total_size` and ETag = blob id, Partial with the exact bytes three times, Full twice, `RangeNotSatisfiable`, Full and `NotModified`

**AC-2: Users never get non-current revisions**
Given a file with an older retained revision
When the old revision is opened without and with `retained_ok`
Then the first is `NotFound` and the second is served

**AC-3: Gone, folders and foreign ids (AC-59, AC-77)**
Given a folder, a tombstoned file and another account's file, and a file whose current revision changes
When each is opened by node, and the changing file is opened again after its new commit
Then the first three are `NotFound`, and `NodeRef(current)` serves the new current revision

**AC-4: Inline only for the allowlist (AC-47)**
Given files whose sniffed types are jpeg, png, gif, webp, avif, mp4 and webm, and html, svg, xml, js, pdf and unknown
When each is opened
Then the first seven are `Inline` and the rest `Attachment`, whatever their names

**AC-5: Corruption ends the stream (AC-7)**
Given a 10 MiB blob with one byte flipped on disk in chunk 40
When it is read in full, and a range inside chunks 0 to 39 is read
Then the full read yields bytes equal to the source up to chunk 40 and then an error, never a complete-looking short body, and the range read succeeds

**AC-6: Missing blob is not a server error**
Given a revision whose blob file was deleted on disk
When it is opened
Then the result is `ContentUnavailable`, no server-class error occurs, and the blob is marked unavailable

## Non-Functional Requirements

**Performance**
- 4 KiB range p95 (open plus first chunk) at most 4 ms (fail 5 ms); streaming at least 250 MB/s (fail 112 MB/s); RSS growth at most 16 MiB (fail 64 MiB) for a 5 GiB stream; measured by PT-03-4 and PT-03-5 in E-14/c14.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Range header parsing and math | One range; multi and overlap ignored; bounds checked |
| AC-4 | Disposition table | 7 inline types; all else attachment |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 5 MiB file | Range and conditional matrix (IT-03-13) | Full/Partial/416/304 as listed | FT-P-42, FT-P-68 |
| AC-2 | Retained revision | With and without `retained_ok` (IT-03-13) | `NotFound`; served | |
| AC-3 | Folder, tombstone, foreign | Open by node (IT-03-13) | `NotFound`; follows current | FT-N-44 |
| AC-4 | 13 sniffed types | Disposition (IT-03-13) | 7 Inline, 6 Attachment | FT-N-11, FT-P-39, NFT-SEC-19 |
| AC-5 | Flipped byte in chunk 40 | Full and inner range (IT-03-14) | Error at chunk 40; range ok | FT-N-02 |
| AC-6 | Blob file deleted | Open (IT-03-14) | `ContentUnavailable`; marked | |

## Constraints

- Real 02 reader and 04 reader on a temp directory; corruption is made on disk outside the service.
- The proxy-level streaming check is pipeline AZ-3657.

## Risks & Mitigation

**Risk 1: Range amplification**
- *Risk*: many ranges multiply work and memory.
- *Mitigation*: one range only; ST-03-3 in E-14/c13.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Where `suggested_name` comes from for `open_blob_content` (its `BlobRef` carries no name) is not stated | E-14 owner | open | Low |
| G-10 is still listed as an owner item in `tests/spec-gaps.md` while 03 decides inline by sniffed type | Owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/03_transfer/download_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
