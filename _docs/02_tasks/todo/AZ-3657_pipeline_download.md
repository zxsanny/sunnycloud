# Download pipeline (F6)

**Task**: AZ-3657_pipeline_download
**Name**: Download pipeline
**Description**: The content route streams decrypted revisions with Range, ETag and serving rules through transfer, tree and blobstore, end to end against the real components and through Caddy
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3801_crate_skeleton_accept_loop, AZ-3812_content_header_mapper, AZ-3829_tree_transfer_feed_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3755_transfer_download, AZ-3707_tree_reader, AZ-3725_master_keyring, AZ-3728_blob_reader, AZ-3655_pipeline_large_upload, AZ-3652_initial_structure; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3657
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p05

## Problem

A download crosses 11 (route, header mapper), 03 (`DownloadService`), 02 (`TreeReader`) and 04 (`BlobReader`, `MasterKeyring`), then Caddy with buffering off. 03 reaches 02 and 04 only through ports bound by 12. No planned task proves that a real request through the real stack returns the right bytes, headers and failure behaviour.

## Outcome

- A committed file is downloadable in full and by Range through the real `serve` process, with the serving rules of AC-47 and clean failure on corruption.
- Every component operation of flow F6 has a production caller.
- At least one integration test runs F6 through the real binary, and one check runs through the test stack's Caddy.

## Scope

### Included
- Flow order and seams (`system-flows.md` F6 steps 1-6):
  1. Revision metadata route → `TreeReader::get_revision`.
  2. Content route with optional `Range`, `If-Range`, `If-None-Match` → `DownloadService::open_revision_content` (node or revision ref).
  3. 03 → `TreeReader::resolve_live_file` or `get_revision` → `BlobReader` with the data key unwrapped by `MasterKeyring` → seek to the first 64 KiB chunk.
  4. Chunks AEAD-verified and streamed → content-serving header mapper (11): 200/206, `Accept-Ranges`, `ETag`, inline only for the allowlist, otherwise attachment with `nosniff` and sandbox CSP.
  5. Caddy forwards with buffering off (release Caddyfile in the test stack).
- The end-to-end integration test of the steps above.

### Excluded
- Client-side verification and save strategies (E-23/c11, E-26/c12), derived and share content (E-22/p09, E-22/p10), scrub findings (E-22/p14).

## Acceptance Criteria

AC-1..AC-6 are local to this task. No interaction risk of `interaction-risks.md` is owned here (AC-72 is split between E-22/p08 and E-22/p13).

### Wiring and flow criteria

**AC-1: Full and ranged reads**
Given a committed 1 GiB file
When it is downloaded in full, then a 4 KiB Range from the middle is read, then a Range past the end, then a request with its ETag in `If-None-Match`
Then the full body hash equals the file hash, the Range returns 206 with the exact bytes, the bad Range returns 416, and the conditional request returns 304

**AC-2: Serving rules**
Given a committed PNG and a committed HTML file
When each is downloaded
Then the PNG is served inline with `nosniff`, and the HTML is served as attachment with `nosniff` and the sandbox CSP

**AC-3: Corruption at rest fails cleanly**
Given a committed file with one byte flipped in a stored chunk (through `e2e-ctl` or a direct volume edit in the test)
When the file is downloaded
Then the stream ends with an error and never as a complete short body, the blob is flagged, and the error is logged with ids only

**AC-4: Streaming through Caddy**
Given the test stack with the release Caddyfile
When a 4 KiB Range and a 1 GiB full read go through Caddy
Then the bytes equal the direct reads, and the first byte of the full read arrives before the server has read the whole blob

**AC-5: Gone and retained content**
Given a deleted file and a file with an older retained revision
When a user requests the deleted file and the old revision
Then both get 404 with no body bytes

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `DownloadService::open_revision_content`, `TreeReader::resolve_live_file` and `get_revision` (from 03), the `BlobReader` open and read path (from 03), the data-key unwrap through `MasterKeyring`, and the content-serving header mapper (from the content route)

## Non-Functional Requirements

**Performance**
- A 4 KiB Range read costs at most 5 ms at p95 (AC-4); measured in E-31/c43.

**Compatibility**
- Caddy timeouts and body settings come from the release Caddyfile, not a test copy.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named operation has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Committed 1 GiB file | Full, Range, bad Range, If-None-Match | Hash equal, 206 exact, 416, 304 | AC-4 |
| AC-2 | PNG and HTML files | Download | Inline+nosniff; attachment+nosniff+CSP | AC-47 |
| AC-3 | One flipped byte in a chunk | Download | Stream error, never complete; blob flagged | AC-7 |
| AC-4 | Test stack with release Caddyfile | Range and full read through Caddy | Same bytes, streamed | |
| AC-5 | Deleted file; retained old revision | Download | 404 | |

## Constraints

- Owner: component 12 (`src/server/`): bindings for this flow and its end-to-end tests among 12's integration tests; the Caddy check runs in the test stack of E-31/c05. No SQL, blob I/O or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume; no sunnycloud component is faked.
- Downstream: E-31/c23 (FT-P-02), E-31/c28 (FT-P-42), E-31/c31 (FT-N-02, FT-N-11, FT-N-12) should depend on this task.

## Risks & Mitigation

**Risk 1: Proxy buffers the body**
- *Risk*: a Caddy setting buffers responses and breaks the heap bound of web downloads.
- *Mitigation*: AC-4 measures time to first byte through Caddy.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Caddyfile lives under `deploy/` (E-00); a needed change goes through an E-00 or owning-epic task, not this one | module-layout repository paths | accepted-risk | Low |
