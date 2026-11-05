# Transfer security suite

**Task**: AZ-3758_transfer_security_suite
**Name**: Transfer security suite
**Description**: Run the transfer IDOR, upload-protocol abuse, Range abuse, hostile content type and quota/disk DoS tests at the component boundary
**Complexity**: 3 points
**Dependencies**: AZ-3734_transfer_start_upload, AZ-3735_transfer_put_block, AZ-3736_transfer_resume_abort, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3755_transfer_download; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 03_transfer
**Tracker**: AZ-3758
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md`, `download_service.md` — the operations under test.

## Problem

The transfer service is the main surface for hostile input: other accounts' ids (AC-59), malformed blocks that could corrupt data or reuse a nonce, thousands of ranges, hostile files that a browser might render (AC-47), and declarations that try to fill the disk (AC-39). Each operation task covers its own rules; this task runs the attack-shaped suites across them.

## Outcome

- 0 successful cross-account calls; same keys in another account create independent rows.
- No rejected block ever rewrites stored ciphertext; no multipart or amplified response; 0 inline hostile types; free space never below the floor.

## Scope

### Included
- ST-03-1 to ST-03-5.

### Excluded
- The HTTP-level matrices (E-21/c18) and system scenarios NFT-SEC-18 to 21 (E-31).

## Acceptance Criteria

**AC-1: IDOR across accounts (AC-59)**
Given account A with a session, a staged row, a receipt and a revision
When B calls `get_upload`, `put_block`, `abort_upload` and `commit_upload` with A's `upload_id`, opens A's revision and blob, and uses the same `client_key`, `staged_key` and `commit_id` as A
Then B gets `UploadNotFound` or `NotFound`, B's same keys create independent rows, A's state is identical, and B never receives A's receipt response

**AC-2: Protocol abuse never rewrites data (AC-7, AC-38)**
Given a session with acked blocks
When B sends a wrong hash, a short body, a long body, a chunked body, other bytes for an acked index, an out-of-range index, `declared_size = 0` on the large path, `declared_size = 2^63`, and a block after commit
Then each gets a typed error and the hash of every file region is unchanged after each rejection

**AC-3: Range abuse cannot amplify**
Given a 1 GiB file
When 10,000 comma-separated ranges, overlapping ranges and 1,000 concurrent `bytes=0-` reads are requested
Then multi and overlapping ranges give one Full stream, there are 0 multipart responses, and RSS growth stays at most 64 MiB with 1,000 streams started

**AC-4: Hostile content types (AC-47)**
Given the DS-17 hostile files (html, svg with script, xml, js, pdf, GIF89a-HTML polyglot, SVG `onload`) and positives (jpg, png, webp, gif, mp4), uploaded under benign names like `a.jpg` and hostile names like `a.html`
When each is committed and downloaded
Then 12 of 12 hostile files are `Attachment` and 5 of 5 positives are `Inline`

**AC-5: Quota and disk DoS (AC-39)**
Given 100 concurrent starts declaring 50 GiB from one account and from 20 accounts, and a disk filler leaving the floor + 1 GiB
When all start and then send blocks until refused
Then each account has at most 8 sessions, the others get `SessionCapReached` or `QuotaExceeded`, `FreeSpaceFloor` stops block writes before the floor, reserved stays at most the quota, and free bytes never drop below the floor

## Non-Functional Requirements

**Performance**
- AC-3 memory bound as stated; the service bounds read-ahead by `blob_read_ahead_chunks` (04).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (suite task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A's session, staged, receipt, revision | B's calls (ST-03-1) | 0 successes; independent rows | NFT-SEC-13 |
| AC-2 | Acked blocks | Nine abuse inputs (ST-03-2) | Typed errors; regions unchanged | NFT-SEC-20 |
| AC-3 | 1 GiB file | Range floods (ST-03-3) | One stream; <= 64 MiB | NFT-SEC-18 |
| AC-4 | DS-17 | Commit and download (ST-03-4) | 12/12 attachment; 5/5 inline | NFT-SEC-19, FT-N-11 |
| AC-5 | DS-26 shape, disk filler | Starts and blocks (ST-03-5) | Caps hold; floor never crossed | NFT-RES-LIM-01, NFT-RES-05 |

## Constraints

- Component runner (env class C) with a real blob volume; the disk filler runs on a dedicated test volume.

## Risks & Mitigation

**Risk 1: Shared runner disk**
- *Risk*: AC-5 fills the runner's disk.
- *Mitigation*: a dedicated small test volume.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `declared_size = 0` on the large path has no named error (see E-14/c04) | E-14 owner | open | Low |
| ST-04-8 (04's own rewrite rule) is open (E-10/c01); AC-2 checks only that 03 never calls a rewrite | Owner via E-10 | open | Low |
