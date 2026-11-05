# Download strategies and outcome labels

**Task**: AZ-3942_download_strategies
**Name**: Download strategies and labels
**Description**: `TransferService.download` picks the strategy per browser and size, verifies the hash in the browser where it can, and always ends with a visible `VerifiedSaved`, `SavedUnverified` or `Failed` label; a mismatch never ends as a good file
**Complexity**: 5 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3931_s12_verified_download_spike, AZ-3935_spa_scaffold, AZ-3939_transfer_service_small_path, AZ-3950_playwright_web_harness, AZ-3969_sync_core_wasm_vectors, AZ-3755_transfer_download, AZ-3829_tree_transfer_feed_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3942
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — `download` and `DownloadOutcome`, produced by E-26/c09.
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/03_transfer/download_service.md` — content and range answers, produced by E-14.
- `_docs/02_document/contracts/11_http_api/content_serving_headers.md` — content headers, produced by E-21/c10.
- `_docs/02_document/contracts/13_sync_core/wasm_exports.md` — block hashing and roll-up, produced by E-23/c16.

## Problem

AC-7 requires end-to-end hash verification, in the browser up to 2 GiB for Firefox and Safari and with the text "not verified in the browser" above that; AC-51 caps the download heap at 200 MB for 50 GiB. Each browser offers different save mechanisms (16 §5), and a corrupt file must never be reported as good.

## Outcome

- Chromium streams to disk with incremental hashing and verifies at any size.
- Firefox and Safari verify up to `web.hash_verify_max_ff_safari` (2 GiB) with the mechanism recorded by E-26/c02; above that they use the browser's own download and show the label and the server's file hash.
- Every download ends with a labelled outcome in the transfers panel (mockup `ui_mockups/web-files.html`: "Verified — Downloaded and checked in the browser.", "Not verified — Saved by the browser without a check (over 2 GB in this browser). Compare its hash:").

## Scope

### Included
- `download(node id or share token, optional revision)` returning `DownloadOutcome` per the contract.
- Strategy 1: Chromium save picker, streamed to disk, hashed block by block with the WASM hasher.
- Strategy 2: Firefox and Safari up to 2 GiB with the E-26/c02 mechanism.
- Strategy 3: above 2 GiB in Firefox and Safari, the browser's native download of the API URL, the "not verified in the browser" label, the server's file hash and a copy action.
- Mismatch handling: `Failed(HashMismatch)`, partial file discarded where the browser API allows, never `VerifiedSaved`.
- Typed failures `Forbidden`, `NotFound`, `Network`; labels and actions in the transfers panel.

### Excluded
- The Download row action wiring in the file browser and the public page button (E-26/c08 and E-26/c13 call `download`); 50 GiB heap runs (E-26/c19b).

## Acceptance Criteria

**AC-1: Chromium verified at any size (AC-7, IT-16-8)**
Given a 2 GiB file in Chromium
When it is downloaded
Then it streams to the chosen file with per-block hashing, the outcome is `VerifiedSaved` with the "Verified" label, and the saved file's hash equals the server's

**AC-2: Firefox and Safari up to 2 GiB are verified (AC-7, IT-16-8)**
Given the mechanism recorded by E-26/c02 and a 1 GiB file
When it is downloaded in Firefox and in Safari
Then the outcome is `VerifiedSaved`, the saved hash equals the server's and the heap stays at or below 200 MB; for an engine where E-26/c02 recorded no passing mechanism, the outcome is `SavedUnverified` with the label instead

**AC-3: Above 2 GiB is labelled unverified (AT-16-2)**
Given a 3 GiB file in Firefox and Safari
When it is downloaded
Then the browser's own download runs, the outcome is `SavedUnverified`, and the panel shows "not verified in the browser" with the server's file hash and a copy action that copies exactly that hash

**AC-4: Threshold boundary**
Given `web.hash_verify_max_ff_safari` 2 GiB
When files of exactly 2 GiB and 2 GiB + 1 byte are downloaded in Firefox
Then the first takes the verified path and the second the labelled unverified path

**AC-5: A corrupt block is never saved as good (AC-7, NFT-SEC-21, AT-16-2 step 3)**
Given a server copy with one corrupt block (seeded on the volume)
When it is downloaded through each verified path
Then the outcome is `Failed(HashMismatch)`, no saved file is labelled verified, and the partial file is removed where the browser allows it

**AC-6: Non-current revision and missing file**
Given the id of a non-current revision as a non-admin, and a deleted file
When each is downloaded
Then the outcome is `Failed(Forbidden)` or `Failed(NotFound)` with its message and nothing is saved

**AC-7: Heap at 5 GiB**
Given a 5 GiB download in Chromium and Firefox
When the heap is sampled every 5 s
Then it stays at or below 200 MB (target 160 MB)

## Non-Functional Requirements

**Performance**
- Download heap target 160 MB, failure above 200 MB (AC-51); 50 GiB in E-26/c19b.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1..AC-4 | Strategy selection by engine features and size, threshold from config | Expected strategy per case |
| AC-5 | Incremental verification with one flipped block | `HashMismatch`; never verified |

## Blackbox Tests

Playwright against the real stack; Safari on the owner Mac (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | 2 GiB (Chromium); 1 GiB (Firefox, Safari) | IT-16-8; FT-P-68 | Verified; hash equal; heap bound | AC-7, AC-51 |
| AC-3, AC-4 | 3 GiB; 2 GiB and 2 GiB + 1 | AT-16-2 steps 1-2; boundary | Labelled unverified with hash; boundary paths | AC-7 |
| AC-5 | One corrupt block | AT-16-2 step 3; NFT-SEC-21 | `HashMismatch`; nothing called verified | AC-7 |
| AC-6 | Non-current revision; deleted file | Typed failures | `Forbidden` or `NotFound` | |
| AC-7 | 5 GiB | Heap sampling | <= 200 MB | AC-51 |

## Runtime Completeness

- **Named capability**: in-browser hash verification with the `sync-core-wasm` hasher, and the S12 save mechanism for Firefox and Safari.
- **Production code that must exist**: the three strategies of 16 §5 over the real content routes.
- **Allowed external stubs**: none; the corrupt block is seeded on the real volume.
- **Unacceptable substitutes**: a JavaScript hash library, trusting `Content-Length` or a status code as verification, reporting `VerifiedSaved` without comparing hashes.

## Constraints

- No whole-file `Blob` in the heap on any path (16 §5).
- The label text "not verified in the browser" is fixed by AC-7.

## Risks & Mitigation

**Risk 1: S12 finds no mechanism for one engine**
- *Risk*: Firefox or Safari cannot verify up to 2 GiB with a bounded heap.
- *Mitigation*: AC-2's fallback keeps the outcome honest (`SavedUnverified` with the label); the owner decides at gate S12.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Firefox and Safari verified path depends on the E-26/c02 decision (S12, R30) | E-26/c02, owner (gate S12) | open | Medium |
| Removing a partial file is possible only where the browser API allows it (16 §5); the record of E-26/c02 states per engine what remains | E-26 owner | accepted-risk | Low |

## Contract

This task implements the `download` part of the contract at `_docs/02_document/contracts/16_web_app/transfer_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
