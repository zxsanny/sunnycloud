# Verified ranged download with resume

**Task**: AZ-3964_verified_download
**Name**: Verified download
**Description**: The `download` operation that fetches a revision with HTTP ranges, verifies every 2 MiB block against the revision blocklist and the whole file against `content_hash`, hands only verified bytes to the host sink, and resumes at the last verified block boundary
**Complexity**: 3 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3959_http_sync_client_connect, AZ-3960_sync_tree_operations, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3964
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `download` (this task contributes it).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `DownloadSink` (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `RevisionRef`, `RevisionView` (E-23/c04).
- `_docs/02_document/contracts/11_http_api/content_serving_headers.md` — Range, 206, 416, stream abort (draft, E-21/c10).
- `_docs/02_document/contracts/03_transfer/download_service.md` — content semantics.

## Problem

Every transferred file must be hash-verified end to end with 0 undetected corruptions (AC-7). A download with one corrupt block must fail and must never hand that block to the host as good data (FT-N-02); a reset in the middle of a 5 GiB file must not restart from zero (NFT-RES-02). The desktop applier (14) writes only what the sink receives, so the verification must happen in 13 before the sink.

## Outcome

- `download(RevisionRef, Arc<dyn DownloadSink>, resume_from) -> Verified{file_hash}` as in the `sync_api` contract.
- Rolling BLAKE3 per block against `get_revision`'s blocklist; whole-file rollup against `content_hash`; `finish(verified = true)` only on a full match.
- Resume from a block boundary; after a reset at most one unverified block is read again.

## Scope

### Included
- Ranged GET of the revision content; reading the blocklist first (`get_revision`).
- Per-block verification before `write_verified`; `HashMismatch{block}` on the first bad block, with no bytes of that block delivered.
- `resume_from` aligned to a block boundary; a reset continues from the last verified block.
- Typed errors `NotFound`, `BlobUnavailable` (404 `content_unavailable`), `Io` (sink), the common errors; a stream that ends early is never treated as complete.

### Excluded
- The real-server leg of IT-13-5 and NFT-RES-02 (E-23/c18a).
- Where and how the host writes the file (14's applier).

## Acceptance Criteria

**AC-1: Corrupt block never reaches the sink (AC-7, FT-N-02)**
Given a scripted fake serving a 40 MiB revision with one bit flipped in block 7 and a `DownloadSink` spy
When `download` runs
Then it returns `HashMismatch{block: 7}`, the sink's offsets cover blocks 0 to 6 only, and `finish(verified = true)` is never called

**AC-2: Resume after the failure**
Given the same revision served clean
When `download` runs again with `resume_from = 7 x 2 MiB`
Then the first range requested starts at 7 x 2 MiB and the result is `Verified{file_hash}` equal to `content_hash`

**AC-3: Reset costs at most one block (NFT-RES-02)**
Given a fake that resets the stream in the middle of block 1,280 of a 2,560-block file
When the download continues
Then the bytes requested after the reset are at most the remaining blocks plus 2 MiB, each block reaches the sink once, and the result is `Verified`

**AC-4: Early end is not success**
Given a fake that closes the stream (h2 reset or connection close) before the declared length
When the download reads it
Then the call does not return `Verified` and the sink's `finish(verified = true)` is not called

**AC-5: Missing content**
Given fakes answering 404 `not_found` and 404 `content_unavailable`
When `download` runs
Then the results are `NotFound` and `BlobUnavailable`

## Non-Functional Requirements

**Performance**
- Memory: one block buffer plus hash state per download; no whole-file buffer (AC-52).

**Reliability**
- The whole-file rollup is checked before `finish(verified = true)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Resume offset alignment for arbitrary `resume_from` values | Aligned down to a block boundary |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake (E-23/c17), bit flip in block 7, sink spy | Download | `HashMismatch{7}`; blocks 0..6 only | IT-13-4, FT-N-02, AT-13-2 step 2 |
| AC-2 | Clean fake | Resume at block 7 | `Verified` | IT-13-4 |
| AC-3 | Fake reset mid-file | Continue | <= one block re-read; each block once | IT-13-5, NFT-RES-02, FT-P-02 |
| AC-4 | Fake early close | Download | Not verified | `content_serving_headers.md` stream-error |
| AC-5 | Fake 404 variants | Download | `NotFound`; `BlobUnavailable` | AC-73 |

## Constraints

- Only verified bytes ever reach the host (13 §7).
- The revision content route serves current revisions to non-admin principals (`canonical-names.md`); a superseded revision is `NotFound`.

## Risks & Mitigation

**Risk 1: Superseded revision mid-download**
- *Risk*: another device commits while a large download runs and the revision stops being current.
- *Mitigation*: the typed `NotFound` lets the host re-read the node (14 owns the retry policy).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| A 416 on a resume beyond the file length has no row in the wire table | E-23 | open | Low |
