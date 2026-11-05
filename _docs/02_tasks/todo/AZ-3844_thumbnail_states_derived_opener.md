# Thumbnail states and derived file lookup

**Task**: AZ-3844_thumbnail_states_derived_opener
**Name**: Thumbnail states and derived opener
**Description**: Report thumbnail state for many files in two indexed probes, and resolve a node or an authorised live file to its derived blob, never falling back to the original
**Complexity**: 3 points
**Dependencies**: AZ-3826_media_gaps_spike, AZ-3839_media_tables_config, AZ-3840_job_queue, AZ-3707_tree_reader, AZ-3729_blob_registry_gc_queue
**Component**: 08 media_jobs
**Tracker**: AZ-3844
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `resolve_live_file` (E-13/c03).
- `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md` — `BlobCatalog::get_ref` (E-10/c09).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `ThumbnailStateSource`, `DerivedOpener` signatures (E-00/c05).

## Problem

Folder listings and the web poll need per-file thumbnail state without a request per file and without slowing the 10,000-entry listing. Thumbnail and transcode routes, including share links, need the derived blob of the current revision; an SVG or any file without a derived row must never be served in its place (AC-47).

## Outcome

- `ThumbnailStateSource::states(account, [content_hash])` returns `Ready`, `Pending`, `Failed` or `None` per hash using one batched probe of the derived key (`thumb`) and one of the job key; `None` when no job is planned for the type.
- `DerivedOpener::open_derived(account, node, kind)` resolves the live file through the tree, then the derived row for its content hash, then the blob reference; it returns `NotReady` while a job is pending and `NotFound` otherwise.
- `open_derived_for(live_file, kind)` does the same for a live file already authorised by 06 for a share link.
- 08 does not authorise visitors and does not stream; 11 passes the blob reference to 03's download service.

## Scope

### Included
- Both ports and the contract file `contracts/08_media_jobs/derived_access.md`.

### Excluded
- Listing enrichment (E-13/c14); routes and the 1,000-id body limit (E-21/c14); share-token checks (E-16/c06).

## Acceptance Criteria

**AC-1: States in two probes (AC-10)**
Given 1,000 content hashes: 400 with a `thumb` row, 300 with queued or running jobs, 100 with dead jobs and 200 SVG or unsupported, plus a vault file's hash and an unknown hash
When `states` is called
Then the map matches the seeded states exactly (vault and unknown are `None`) and the call makes 2 statements

**AC-2: Never a fallback (AC-47)**
Given a photo with a finished thumbnail, a photo whose job is queued, an SVG, a vault file, a compliant video without transcode, and another account's node
When `open_derived` is called for each
Then the ready photo returns a reference that reads back a WebP, the queued photo returns `NotReady`, the SVG, vault file, compliant video's transcode and foreign node return `NotFound`, and `open_derived_for` on the same live file equals `open_derived`

**AC-3: No cross-account state (AC-59)**
Given account A's hashes and node ids
When account B calls `states` with A's hashes and `open_derived` with A's node
Then no state of A is reported (per the E-18/c01 answer) and the lookup returns `NotFound`

## Non-Functional Requirements

**Performance**
- The two probes are index lookups with no join into the listing query; NFT-PERF-10 includes them.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Merge of the two probe results into states | Ready, Pending, Failed, None as listed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 1,000 seeded hashes | IT-08-14 | Exact map; 2 statements | NFT-PERF-10 |
| AC-2 | Six cases | IT-08-13 | Derived, `NotReady`, `NotFound`; never the original | NFT-SEC-19, FT-P-39 |
| AC-3 | Accounts A and B | ST-08-4 port part | Nothing of A visible | NFT-SEC-13 |

## Constraints

- Derived output belongs to content within an account: copies and restored revisions reuse it.

## Risks & Mitigation

**Risk 1: Listing latency (08 risk 4)**
- *Risk*: the probes slow the listing.
- *Mitigation*: two batched index probes; the separate `thumbnail-states` poll is the fallback.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-08-3 (foreign ids in `thumbnail-states`, status above 1,000 ids) is decided by E-18/c01 | E-18 owner | open | Low |
| The `thumbnail-states` route takes node ids, but `states` takes content hashes; who resolves node ids to hashes (11 through `TreeReader`) is not stated | E-18 / E-21 owners | open | Low |
| A `BlobRef` carries no media type; the derived row's media type must reach 03's download service (contracts part 3 open point 6) | E-18 / E-21 owners | open | Low |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/derived_access.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
