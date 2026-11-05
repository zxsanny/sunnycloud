# Web transfer pipeline (F4, F5, F6 and F10 thumbnails from the browser)

**Task**: AZ-3671_pipeline_web_transfer
**Name**: Web transfer pipeline
**Description**: The file browser's uploads, resumes, thumbnails and downloads run through the transfer facade, the module Worker and the WASM core to the real API and back, end to end in a real browser
**Complexity**: 3 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3935_spa_scaffold, AZ-3938_file_browser, AZ-3939_transfer_service_small_path, AZ-3940_worker_large_upload, AZ-3941_resume_edit_detection, AZ-3942_download_strategies, AZ-3945_thumbnail_polling, AZ-3950_playwright_web_harness, AZ-3952_browser_50gib_runs, AZ-3969_sync_core_wasm_vectors, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3657_pipeline_download, AZ-3661_pipeline_media_derived_outputs, AZ-3652_initial_structure; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3671
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/p01

## Problem

The web app's transfer path is its own client loop: pages hand files to `TransferService`, which chooses the small-file path or the module Worker (slicing, WASM hashing and planning, up to four lanes), keeps resume records in IndexedDB, polls thumbnail states and picks a download strategy. E-26 splits this across five children, each with its own Playwright tests. No planned task owns the composition from the file browser through the facade to the real server, or proves the full round trip in one run.

## Outcome

- Files dropped in the file browser upload through the right path, survive a tab reload, show thumbnails when ready and download verified, against the real server behind the release Caddyfile.
- Every component operation of the web transfer path has a production caller from the pages through the facade.
- At least one Playwright test runs the round trip against the real stack.

## Scope

### Included
- Flow order and seams (web side of F4, F5, F6 and F10):
  1. File browser page → `TransferService::enqueueUpload(files, folder)`; files up to `small_file_max` (from `ServerInfo`) → staging plus one batch commit; larger files → module Worker `start{file, parent, name}`.
  2. Worker → `sync-core-wasm` exports (block hash, roll-up, commit id, resume diff) → up to four block lanes through the generated `ApiClient` → commit.
  3. IndexedDB resume record → after a reload the user re-selects the file → `TransferService::resumeUpload` (size and `lastModified` must match).
  4. Listing refresh; thumbnail states polled every 2 s while visible → thumbnail image.
  5. `TransferService::download(node)` → streaming save where available, in-browser verification up to 2 GiB → `VerifiedSaved` or `SavedUnverified`.
- The page-to-facade composition (pages import only the `TransferService` and `ApiClient` facades) and the Playwright round trip.

### Excluded
- Auth, approval, share and admin pages (their own E-26 children, tested against the real server), the vault page (v1.2), server pipelines (E-22).

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction risk of `interaction-risks.md` lies on the web transfer path.

### Wiring and flow criteria

**AC-1: Drop, upload, list**
Given a signed-in browser on the real stack
When 20 small files and one 300 MiB file are dropped on a folder in the file browser
Then the small files commit in one batch, the large file uploads over at most four lanes with WASM hashing, the listing shows all 21, and the page heap stays at or below 150 MB

**AC-2: Resume after a tab reload**
Given the 300 MiB upload at about half
When the tab is reloaded and the same file is selected again, and in a second run a changed file is selected
Then the first resumes and resends at most 10 MiB, and the second restarts with the "changed since last time" message

**AC-3: Thumbnails appear**
Given a photo dropped in the file browser
When the upload commits
Then a placeholder shows first and the thumbnail replaces it once its state is ready, without a page reload

**AC-4: Verified download**
Given a committed file
When it is downloaded through the file browser in Chromium, and a second file is served with a corrupted block (seeded on the volume)
Then the first is saved with the "verified" label and the second ends with the failed label and no saved file

**AC-5: Every flow operation has a production caller**
Given the web source tree
When the wiring check searches non-test code
Then pages import only the `TransferService` and `ApiClient` facades, the file browser calls `enqueueUpload`, `resumeUpload` and `download`, `TransferService` routes by `small_file_max` from `ServerInfo`, the Worker calls the `sync-core-wasm` exports, and the thumbnail component polls the thumbnail-states route

## Non-Functional Requirements

**Performance**
- Page heap at most 150 MB while uploading and 200 MB while downloading (AC-50, AC-51); 50 GiB runs are E-26/c19 on the lab host.

**Compatibility**
- Chromium and Firefox in CI; Safari through the lab runner (reported NOT-RUN when absent).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Import and caller check over the web sources | Pages use only the facades; each named call has a production caller |

## Blackbox Tests

Playwright in the `browsers` container against the real stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in browser | Drop 20 small + 1 large file | One batch, four lanes max, 21 listed, heap bound | AC-50 |
| AC-2 | Half-done large upload | Reload, re-select same and changed file | Resend at most 10 MiB; restart message | AC-3 |
| AC-3 | Photo | Drop and wait | Placeholder then thumbnail | AC-10 |
| AC-4 | Good file; corrupted block | Download | Verified label; failed label, no file | AC-51 |

## Constraints

- Owner: component 16 (`src/web/`): the page-to-facade composition and this pipeline's Playwright tests.
- Tests run against the real server behind the release Caddyfile (E-26 testing strategy); no mocked API.
- Downstream: E-31/c30 (web scenarios of FT-P-62..74) and E-31/c45 should depend on this task.

## Risks & Mitigation

**Risk 1: Pages bypass the facade**
- *Risk*: a page calls the API directly for uploads, creating a third upload implementation.
- *Mitigation*: AC-5 import check.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| S12: verified download up to 2 GiB in Firefox and Safari with a bounded heap is still a spike (E-26/c02) | E-26 | open | Medium |
| `TooManyFiles` trigger limit is undecided (E-26/c05) | E-26 | open | Low |
