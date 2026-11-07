# Worker large-upload engine: slicing, WASM hashing, four lanes, planner integration

**Task**: AZ-3940_worker_large_upload
**Name**: Worker large-upload engine
**Description**: The module Worker that uploads files above `small_file_max`: 2 MiB slices hashed by `sync-core-wasm`, an upload session from the blocklist, one block per request on up to four lanes, conflicts handed to the WASM planner, commit by roll-up
**Complexity**: 5 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3932_ios_safari_memory_spike, AZ-3935_spa_scaffold, AZ-3939_transfer_service_small_path, AZ-3950_playwright_web_harness, AZ-3969_sync_core_wasm_vectors, AZ-3734_transfer_start_upload, AZ-3735_transfer_put_block, AZ-3738_transfer_commit_upload, AZ-3829_tree_transfer_feed_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3940
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — Worker protocol and lane rule, produced by E-26/c09.
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper, produced by E-26/c06.
- `_docs/02_document/contracts/13_sync_core/wasm_exports.md` — block hash, roll-up, commit ID and resume-diff exports, produced by E-23/c16 (written by the E-23 batch).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `BLOCK_SIZE`, effective lanes, `UploadPlanner`, produced by E-23/c04.
- `_docs/02_document/contracts/03_transfer/upload_service.md` — sessions, block PUT, `X-Blake3`, produced by E-14.

## Problem

A browser must upload files up to 50 GiB with at most 150 MB of heap (AC-50) and identical hashing and planning rules to the server (ADR-004). The browser cannot run the Rust HTTP client, so the Worker drives `fetch` itself and asks the WASM module for every rule that must match byte for byte; the page's main thread must stay responsive.

## Outcome

- Files above `small_file_max` upload through the Worker with at most `min(4, max_lanes)` concurrent block requests, one block each.
- Hash, roll-up, commit ID and conflict handling come from `sync-core-wasm`; the server's hash equals the local one.
- The page stays responsive and holds no `File` after hand-off; the transfers panel shows lanes, speed and time left.

## Scope

### Included
- Hand-off by `start{file, parent, name, baseRevision?}`; the page drops its `File` reference.
- Reading 2 MiB slices, hashing each block in WASM, building the blocklist, creating the session with a random UUIDv7 client key.
- Block PUTs with `X-Blake3` through the `ApiClient` wrapper on up to `min(4, max_lanes)` lanes, one block per request, no streaming request bodies; `409 BlockConflict` handed to the planner (re-read session, resend or restart); `NothingToSend` ends with 0 block bytes; commit by roll-up; `done{revision}` or `failed{typedError}`.
- `cancel` stops in-flight requests and aborts the session.
- Progress messages with bytes and lanes; the panel line "N lanes · speed · time left" (mockup "Folder").
- WASM conformance in the browser (IT-16-18) and main-thread responsiveness (PT-16-3).

### Excluded
- Resume records, edit detection and tab-close recovery (E-26/c11); the 50 GiB heap runs (E-26/c19b); downloads (E-26/c12).

## Acceptance Criteria

**AC-1: Lanes and one block per request (AC-4, IT-16-4)**
Given a 5 GiB file (DS-09) and `max_lanes` 4
When it uploads
Then the network log shows at most 4 concurrent block PUTs, each carrying exactly one block (2 MiB, the last shorter) with no streamed body, and the server's file hash equals the local hash

**AC-2: Block conflict goes to the planner**
Given the server answers one block PUT with 409 `BlockConflict`
When the upload continues
Then the Worker re-reads the session and resends or restarts as the planner decides, and the committed content equals the file

**AC-3: Known content sends nothing**
Given content the server already holds for this upload
When it is uploaded
Then the upload ends with `NothingToSend` and 0 block bytes sent

**AC-4: Constant-time finish (AC-4)**
Given the last block acknowledged
When the commit runs
Then the file is published within 2 s

**AC-5: The page holds no `File`**
Given a large upload handed to the Worker
When a heap snapshot of the page context is taken
Then it holds no `File` object of that job

**AC-6: WASM conformance in three engines (IT-16-18, AC-7)**
Given the shared vector file (0, 1, 2 MiB − 1, 2 MiB, 2 MiB + 1, 6 MiB and 100 MiB)
When it is hashed in the module Worker in Chromium, Firefox and WebKit
Then block hashes, roll-ups and commit IDs equal the native vectors

**AC-7: Main thread stays responsive (PT-16-3)**
Given a 5 GiB upload with a scroll and a click every second
When hashing runs
Then the longest main-thread task is at most 50 ms (failure above 100 ms), input latency p95 is at most 100 ms (failure above 200 ms), and no hashing task runs on the main thread (long-task observer)

**AC-8: Cancel and heap at 5 GiB**
Given a 5 GiB upload in Chromium and Firefox
When the heap is sampled every 5 s and the upload is cancelled at 50 % in a second run
Then the page plus Worker heap stays at or below 150 MB; after cancel no further block request is sent and the session is aborted

## Non-Functional Requirements

**Performance**
- Heap target 120 MB, failure above 150 MB (AC-50); 50 GiB runs in E-26/c19b. No idle gap above 1 s between blocks while the link is busy (PT-16-1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Lane scheduler with random request failures | Never more than `min(4, max_lanes)` in flight; no block lost |
| AC-2, AC-3 | Planner outcomes from the WASM module (conflict, nothing to send) | Worker follows the planner's decision |
| AC-6 | Vector file through the built WASM in the unit runner | Equal to native vectors |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-4 | DS-09 (5 GiB); scripted 409; known content | IT-16-4 | Lanes, conflict, 0 bytes, finish <= 2 s | AC-4 |
| AC-5 | Large upload | Page heap snapshot | No `File` on the page | |
| AC-6 | Vector file | IT-16-18 | Equal to native | AC-7 |
| AC-7 | DS-09 with interactions | PT-16-3 | Task and latency bounds | AC-50 |
| AC-8 | DS-09 | Heap sampling; cancel | <= 150 MB; abort | AC-50 |

## Runtime Completeness

- **Named capability**: `sync-core-wasm` block hashing, roll-up, commit ID and `UploadPlanner` resume diff, called from the module Worker.
- **Production code that must exist**: the Worker calling those exports for every upload above `small_file_max`, over the real upload routes.
- **Allowed external stubs**: none in the blackbox tests; the fault proxy only injects statuses.
- **Unacceptable substitutes**: a JavaScript BLAKE3 or hand-written planner, hashing on the main thread, a mocked API.

## Constraints

- One block per request, no streaming request bodies (16 §5, epic Requirements).
- `block_size` other than 2 MiB disables uploads (E-26/c06 AC-7).

## Risks & Mitigation

**Risk 1: Hashing slower than the link**
- *Risk*: WASM BLAKE3 speed varies by browser (16 §7).
- *Mitigation*: hash ahead of the lanes within the heap bound; PT-16-1 checks for idle gaps.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `need_blocks` and `edited_since` are both 409 (resolved 2026-11-07, G-11-1; 412 is for `If-Match`); the Worker switches on the code, not the status | E-21/c02 | resolved | Low |
| A UUIDv7 constructor for the WASM commit ID is open (`sync_core_proto.md` open point 4); either the export provides it or the Worker generates the key | E-23/c16 | open | Low |
| iOS Safari ceilings from E-26/c03 may require a size message on iPhones | E-26/c03, owner | open | Low |

## Contract

This task implements the Worker protocol and the large-file path of the contract at `_docs/02_document/contracts/16_web_app/transfer_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
