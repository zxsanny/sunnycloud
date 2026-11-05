# `TransferService` facade and small-file path: staging, batch commit, queue cap, transfers panel

**Task**: AZ-3939_transfer_service_small_path
**Name**: TransferService and small-file uploads
**Description**: Create the page-facing `TransferService` with `enqueueUpload`, `cancelUpload` and the progress store, the small-file path (staging plus batch commit with a reused commit ID), the in-flight queue cap, file and folder upload controls and the transfers panel
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3934_too_many_files_spike, AZ-3950_playwright_web_harness, AZ-3708_tree_folder_mutations, AZ-3737_transfer_stage_small, AZ-3754_transfer_commit_batch, AZ-3829_tree_transfer_feed_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3939
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/03_transfer/upload_service.md` and `commit_service.md` — staging and batch commit, produced by E-14.

## Problem

Most uploads are small files; one request per file and one batch commit keep them fast (AC-40). A retried batch must not create duplicates, a staging cap must not stall the queue, and a 100,000-file selection must not exhaust memory. Pages need one facade and one progress store so no page builds its own upload code.

Points: planned at 3; the approved mockup adds the transfers panel and folder uploads to this row, and it runs PT-16-4, so it is estimated at 5.

## Outcome

- `TransferService` exists with the shape of the contract below; large files are handed to the Worker path (E-26/c10).
- Files up to `small_file_max` are staged and committed in batches; a retry reuses the commit ID and never duplicates.
- At most 200 `File` objects are in flight; a selection above the E-26/c05 limit is refused with `TooManyFiles`.
- Users can upload files and folders and follow every job in the transfers panel (mockup `ui_mockups/web-files.html`, state "Folder").

## Scope

### Included
- Facade methods `enqueueUpload`, `cancelUpload` and the progress store; routing by `small_file_max` from `ServerInfo`.
- Small path: each file staged under its client key, committed in batches of at most `batch_max_entries` with a random UUIDv7 commit ID kept in memory and reused on every retry; `StagedCapReached` → commit what is staged, then stage more; a closed tab abandons the batch.
- Queue cap `web.max_inflight_files` (200) and the `TooManyFiles` refusal at the E-26/c05 limit.
- Typed errors per job with their own message: `InvalidName`, `QuotaExceeded` (507), `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`, `ParentNotFound`, `AccountNotActive`, `TooManyFiles`; other jobs continue.
- Upload controls: pick files, pick a folder (files only), drop files and folders (empty folders only by drag and drop); subfolders created through the API before their files; large trees point to the desktop client.
- Transfers panel: per-job name, bytes and percent, state, "N of M small files" for batches, cancel, dismiss, per-job error lines.

### Excluded
- The Worker large path (E-26/c10), resume (E-26/c11), downloads (E-26/c12), the 50 GiB runs (E-26/c19b).

## Acceptance Criteria

**AC-1: Size boundary routes the path (AC-40, IT-16-3)**
Given files of 2 MiB − 1, 2 MiB and 2 MiB + 1 (DS-23)
When they are enqueued
Then the first two are staged under their client keys and committed in one batch, and the third is handed to the block path

**AC-2: Batches and a retried batch (IT-16-3, FT-N-38 browser side)**
Given 300 small files and `batch_max_entries` 100, and the network cut once right after a batch request
When they upload
Then no batch has more than 100 entries, the retried batch carries the same commit ID and gets the stored response, and exactly 300 files exist afterwards

**AC-3: Staging cap does not stall**
Given `max_staged_per_account` 200 and 300 small files
When the server answers `StagedCapReached`
Then the client commits what is staged, then stages the rest, and all 300 end committed

**AC-4: Typed errors per job (IT-16-7, FT-P-79 step 1)**
Given the fault proxy injects each upload error in turn
When jobs run
Then each failed job shows its own message, the other jobs complete, and a 507 also shows the quota notice with used and limit equal to the quota view

**AC-5: Queue cap and selection limit (PT-16-4)**
Given a selection of 100,000 tiny files, and a selection above the E-26/c05 limit
When they are enqueued
Then at most 200 `File` objects are in flight (heap snapshot count), the first upload starts within 5 s and the heap stays at or below 150 MB (failure above 300 MB); the larger selection is refused with `TooManyFiles` and its message, and nothing uploads

**AC-6: Folder uploads**
Given a picked folder with two nested subfolders and files, and an empty folder dropped on the page
When they upload
Then the subfolders are created before their files and the files land in them; the dropped empty folder is created; an empty folder inside a picked folder is not created (files only)

**AC-7: Cancel**
Given a queued job and a running batch
When each is cancelled
Then the queued job sends no request, the running batch sends nothing further, and files committed before the cancel stay

**AC-8: Transfers panel reflects the store**
Given running, finished and failed jobs
When the panel is open
Then each job shows name, bytes or file count, state and its error line, and a dismissed job leaves the panel without affecting others

## Non-Functional Requirements

**Performance**
- PT-16-4: heap at most 150 MB (failure 300 MB) and first upload within 5 s (failure 15 s) for 100,000 files; 10,000 files of up to 1 MiB within 60 s at 20 ms RTT is the system target (AC-40).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Routing by `small_file_max` at the boundary | Two staged, one to the block path |
| AC-2 | Batch splitting and commit ID reuse on retry | <= `batch_max_entries` per batch; same ID on retry |
| AC-3 | `StagedCapReached` handling | Commit then continue staging |
| AC-5 | Queue cap accounting | Never more than 200 in flight |

## Blackbox Tests

Playwright against the real stack with the fault proxy (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2, AC-3 | DS-23 boundary files, 300 small files, cut after batch | IT-16-3 | Paths, batch sizes, no duplicates | AC-40 |
| AC-4 | Injected errors | IT-16-7; FT-P-79 step 1 | Message per job; quota notice values | AC-39 |
| AC-5 | 100,000 tiny files | PT-16-4 | <= 200 in flight; heap and start time bounds | AC-40 |
| AC-6 | Nested folder; empty dropped folder | Folder upload | Folders created; files placed | |
| AC-7, AC-8 | Mixed jobs | Cancel and panel | As stated | |

## Constraints

- Pages call only `TransferService` for uploads; no page calls staging or commit routes.
- The commit ID and staged keys live in memory for the life of the tab only (16 §5).

## Risks & Mitigation

**Risk 1: Retry after a lost response creates duplicates**
- *Risk*: a fresh commit ID on retry would commit twice.
- *Mitigation*: AC-2 fault test with the cut after the request.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `TooManyFiles` limit comes from E-26/c05 | E-26/c05 | open | Low |
| The status for a batch above the entry limit is open (413 or 422, `problem_details.md` open point 2); the client never sends more than `batch_max_entries`, so the page is not affected | E-21/c02 | accepted-risk | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/16_web_app/transfer_service.md` (facade shape, `enqueueUpload`, `cancelUpload`, `progress`, routing rule).
Consumers MUST read that file — not this task spec — to discover the interface.
