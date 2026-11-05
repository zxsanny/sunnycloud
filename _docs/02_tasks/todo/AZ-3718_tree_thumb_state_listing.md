# Thumbnail state enrichment of listings

**Task**: AZ-3718_tree_thumb_state_listing
**Name**: Tree listing thumbnail states
**Description**: Fill each listing item's `thumb_state` from `ThumbnailStateSource` with one call per page for image and video items only, and `none` when no source is wired
**Complexity**: 2 points
**Dependencies**: AZ-3707_tree_reader, AZ-3770_contracts_part3
**Component**: 02_tree
**Tracker**: AZ-3718
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_reader.md` — this task fills the `thumb_state` field of listing items.

## Problem

The web and mobile grids need to know per item whether a thumbnail is ready, pending, failed or absent, without a request per item. Thumbnail state lives in 08; the tree must ask once per listing page and must not slow the 10,000-entry listing beyond AC-6 (E-13 risk 5).

## Outcome

- One `ThumbnailStateSource::states(account, [content_hash])` call per page, with the hashes of image and video items only.
- Items carry `ready`, `pending`, `failed` or `none`; with no source wired every item is `none` and no extra call is made.

## Scope

### Included
- Enrichment inside `list_children` after the page query.
- Selection of image and video items by their sniffed media type.

### Excluded
- The source implementation (E-18/c09); listing paging (E-13/c03); the performance run (E-13/c15, NFT-PERF-10).

## Acceptance Criteria

**AC-1: One call per page, media items only**
Given the DS-02 folder with 300 image items and a fixed thumbnail-state map
When the folder is listed page by page
Then the source is called once per page, only with content hashes of image and video items

**AC-2: States land on the right items**
Given the fixed map with ready, pending and failed entries
When a page is listed
Then each image or video item carries its mapped state, unmapped media items carry `none`, and non-media items carry `none`

**AC-3: No source wired**
Given a tree built without a `ThumbnailStateSource`
When the folder is listed
Then every item carries `none` and no thumbnail call is made

## Non-Functional Requirements

**Performance**
- Page p95 stays within PT-02-1 targets (40 ms, fail 80 ms) with enrichment on; measured in NFT-PERF-10 (E-13/c15).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Media item selection | Only image/* and video/* hashes passed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-02, 300 images, fixed map | Paged listing (IT-02-16) | One call per page, media hashes only | NFT-PERF-10 |
| AC-2 | Fixed map | Page items | Mapped states; others `none` | |
| AC-3 | No source | Listing (IT-02-16) | All `none`; no call | |

## Constraints

- The fixed map is a per-call port supplied by the test (02 tests.md); the real 08 source runs in E-13/c16.
- The `ThumbnailStateSource` trait comes from `contracts` (E-00/c05).

## Risks & Mitigation

**Risk 1: Second query per page**
- *Risk*: enrichment doubles the listing cost.
- *Mitigation*: one batched call per page; measured in NFT-PERF-10.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
