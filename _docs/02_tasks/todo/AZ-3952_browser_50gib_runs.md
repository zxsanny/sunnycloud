# 50 GiB browser upload and download heap runs

**Task**: AZ-3952_browser_50gib_runs
**Name**: 50 GiB browser runs
**Description**: Run and report the 50 GiB upload (with tab-close resume) and download in Chromium, Firefox and Safari with heap sampled every 5 s, on the large-volume host and the owner Mac
**Complexity**: 3 points
**Dependencies**: AZ-3950_playwright_web_harness, AZ-3939_transfer_service_small_path, AZ-3940_worker_large_upload, AZ-3941_resume_edit_detection, AZ-3942_download_strategies; notes: E-31/c06 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3952
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c19b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — upload, resume and download outcomes, produced by E-26/c09.

## Problem

AC-50 and AC-51 are stated at 50 GiB in three browsers: upload heap at most 150 MB with at most 10 MiB re-sent after a tab close, download heap at most 200 MB with the right label. These runs need the 200 GiB `blobs-large` volume, hours of runtime and the owner Mac for Safari (environment.md C-LARGE, H-MAC).

Split note: second half of epic row 19 (see E-26/c19a for the reason).

## Outcome

- PT-16-1 and PT-16-2 results per engine, with the host, the heap series and the outcome label, in the shared report.
- The resume re-send at 50 GiB measured (NFT-RES-29, FT-P-67).
- Missing hosts reported `NOT-RUN(HW-REF)` or `NOT-RUN(HW-DEVICE)`, never substituted.

## Scope

### Included
- 50 GiB upload (DS-10) in Chromium and Firefox on C-LARGE and in Safari on H-MAC, heap sampled every 5 s, over `shape-wan`; one run with the tab closed mid-way and the file re-selected.
- 50 GiB download in the same engines with heap sampling and the label check.
- Detached long runs with a heartbeat and a STALLED rule; results into the shared report.

### Excluded
- The engines themselves (E-26/c09 to c12); the 5 GiB checks in those tasks.

## Acceptance Criteria

**AC-1: Upload heap at 50 GiB (AC-50, PT-16-1, NFT-PERF-27)**
Given DS-10 (50 GiB) on C-LARGE and on H-MAC
When it uploads in Chromium, Firefox and Safari with heap sampled every 5 s
Then the page plus Worker heap stays at or below 150 MB (target 120 MB), no gap between blocks exceeds 1 s while the link is busy, and the server hash equals the local hash

**AC-2: Resume at 50 GiB (AC-3, NFT-RES-29, FT-P-67)**
Given the same upload with the tab closed at about half
When the tab is reopened and the file re-selected
Then at most 10 MiB is re-sent and the upload completes with an equal hash

**AC-3: Download heap at 50 GiB (AC-51, PT-16-2, NFT-PERF-28)**
Given the 50 GiB file on the server
When it is downloaded in Chromium, Firefox and Safari with heap sampled every 5 s
Then heap stays at or below 200 MB (target 160 MB) and the label is right for each engine (Chromium verified; Firefox and Safari "not verified in the browser" with the server hash)

**AC-4: Honest reporting**
Given the runs
When the report is written
Then each result names engine, host and heap source; Safari values are labelled weak; a run on another host or without the Mac is `NOT-RUN(HW-REF)` or `NOT-RUN(HW-DEVICE)`, never a pass

**AC-5: Buffered test input is labelled (epic risk 6)**
Given an engine where the automation feeds the 50 GiB file through memory
When that is detected (heap rises with the input before any upload)
Then the run is repeated as a manual-driven run on the lab host and labelled so in the report

## Non-Functional Requirements

**Performance**
- Upload heap target 120 MB, failure above 150 MB; download target 160 MB, failure above 200 MB (epic NFR table).

**Reliability**
- Runs are detached with a log and an exit file; no output for 2 minutes is STALLED and aborted (environment.md).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Report mapping for missing hosts | `NOT-RUN` labels, never pass |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | DS-10; C-LARGE; H-MAC; `shape-wan` | PT-16-1; NFT-PERF-27; NFT-RES-29; FT-P-67 | <= 150 MB; <= 10 MiB re-sent | AC-50, AC-3 |
| AC-3 | DS-10 on the server | PT-16-2; NFT-PERF-28; FT-P-68 | <= 200 MB; correct labels | AC-51, AC-7 |

## Constraints

- Results from hosts other than those named are not substituted (environment.md).

## Risks & Mitigation

**Risk 1: Playwright buffers the 50 GiB input**
- *Risk*: Firefox or WebKit automation reads the whole file into memory (epic risk 6).
- *Mitigation*: AC-5 detection and the labelled manual-driven run.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The REF-SRV host with `blobs-large` and a 60 GiB browser scratch volume, and the owner Mac, must exist (E-31/c01 inventory) | Owner (hardware) | open | Medium |
