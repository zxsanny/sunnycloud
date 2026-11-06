# Small upload: stage_small, save-before-stage batch records, pacing, batch_commit

**Task**: AZ-3963_small_upload_batch
**Name**: Small upload and batch commit
**Description**: Single-request staging of files up to `small_file_max`, batch records saved before the first staging, pacing under half the account's staged cap, chunking by entries and bytes, and an idempotent `batch_commit` replayed with the same `commit_id`
**Complexity**: 5 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3963
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `stage_small`, `batch_commit` (this task contributes them).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CheckpointStore` batch records (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `BatchCommit`, `BatchResult`, `check_batch_ids`.
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md` — staged rows, receipts, replay (drafts).

## Problem

AC-40 needs 10,000 files of up to 1 MiB to land in at most 60 s at 20 ms RTT, using one request per file and a batch commit. A kill at any point must not create duplicates: the batch must be saved with its `commit_id` and members before the first member is staged, so a replay sends the same id and gets the stored response (FT-N-38, NFT-RES-22). Other devices share the account's staged cap, so one device must stay under half of it. Without this task, hosts would build batches and pacing themselves.

## Outcome

- `stage_small(staged_key, bytes, hash) -> Staged` and `batch_commit(BatchCommit) -> BatchResult` as in the `sync_api` contract.
- A batch executor that saves `BatchRecord` before staging, stages in parallel over HTTP/2, commits in chunks of at most `batch_max_entries` and `sync.batch_max_bytes_in_flight`, keeps staged-uncommitted files at or below half of `max_staged_per_account`, and replays unfinished batches from `load_batches`.
- The size rule: a file of at most `small_file_max` (2 MiB, 0 bytes included) takes the staged path; a larger one takes the block path.

## Scope

### Included
- `stage_small` with `X-Blake3`, idempotent by key; `staged_key` = the file's `client_key`.
- Batch formation helpers for the host (random UUIDv7 `commit_id` stored with the record, checked against member keys), `save_batch` before the first `stage_small`, `clear_batch` after the host stored the results.
- Replay at start: `load_batches`, same `commit_id` and entries; a staged key the server no longer has is staged again from the source after an identity re-check.
- Pacing: `StagedCapReached` → commit what is staged, then stage more; staged-uncommitted <= half the cap.
- Chunking by `ServerInfo.batch_max_entries` and `sync.batch_max_bytes_in_flight`.
- Per-entry outcomes (`Committed`, `EntryError`) and the batch errors `BatchTooLarge`, `CommitIdReused`.

### Excluded
- The real-server leg of IT-13-7 and PT-13-4 (E-23/c18a, c18b).
- Which files a host groups into a batch and how it stores results (14, 17).

## Acceptance Criteria

**AC-1: Save before stage (AC-40)**
Given a `CheckpointStore` spy over `MemoryCheckpointStore` and 300 files
When the batch runs
Then `save_batch` for each batch is recorded before every `stage_small` of that batch's members

**AC-2: Kills replay the same id (FT-N-38, NFT-RES-22)**
Given kills after `save_batch`, after 150 stagings and after `batch_commit` was sent
When the client restarts and calls `load_batches`
Then each unfinished record is returned, the same `commit_id` and entries are replayed, the fake returns the stored response for the repeated id, the fake holds 300 files with 0 duplicates, and `clear_batch` runs only after the results were handed to the host

**AC-3: Size routing at the boundary**
Given files of 0 bytes, 2 MiB - 1, 2 MiB and 2 MiB + 1 (DS-23 sizes)
When they are routed
Then the first three take the staged path and the 2 MiB + 1 file takes the block path

**AC-4: Chunking and pacing (AC-40)**
Given a fake `ServerInfo{batch_max_entries: 100, max_staged_per_account: 200}`, 1,000 small files, and a fake that answers `staged_cap_reached` at 200 uncommitted
When the batch executor runs
Then no batch exceeds 100 entries, the fake never sees more than 100 staged-uncommitted files from this client, no `BatchTooLarge` occurs, and after a `StagedCapReached` the client commits first and then stages more

**AC-5: Lost staged row is re-staged**
Given a replayed batch whose member key the fake no longer has
When the replay stages it
Then the source identity is re-checked and the file is staged again from the source before the commit

**AC-6: Reused id and per-entry errors**
Given a fake answering `commit_id_reused` for one batch and an `EntryError` for one entry of another
When the batches commit
Then the first returns `CommitIdReused` (the host drops the batch and builds a new one with a new id) and the second returns `Committed` for the other entries and `EntryError{code}` for that one

## Non-Functional Requirements

**Performance**
- Stagings run in parallel over the shared HTTP/2 connection; the AC-40 time bound (<= 60 s, target 45 s) is measured in E-23/c18b (PT-13-4).

**Reliability**
- Memory bounded by `sync.batch_max_bytes_in_flight` (64 MiB) of staged bodies in flight.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Size routing at 0, 2 MiB - 1, 2 MiB, 2 MiB + 1 | Staged, staged, staged, block |
| AC-4 | Chunk planning for entry and byte limits | No chunk above either limit |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Store spy, 300 files, scripted fake (E-23/c17) | Order of calls | `save_batch` first | IT-13-7 |
| AC-2 | Kills at three points | Replay | Same id; 300 files; 0 duplicates | IT-13-7, FT-N-38, NFT-RES-22 |
| AC-4 | Fake caps 100 / 200, 1,000 files | Chunking and pacing | <= 100 per batch; <= 100 staged; commit first | IT-13-8, FT-P-04 |
| AC-5 | Fake missing a staged key | Re-stage | Identity check; staged again | IT-13-7 |
| AC-6 | Fake `commit_id_reused`, entry error | Batch outcomes | Typed results | IT-13-13 |

## Constraints

- `commit_id` is a random UUIDv7 stored with the record; it is never derived from one asset and never equals a member key (13 §3).
- 13 persists nothing itself; batch records live in the host's store.

## Risks & Mitigation

**Risk 1: Another device fills the shared staged cap**
- *Risk*: two devices each stage half the cap and still collide.
- *Mitigation*: `StagedCapReached` is handled by committing first, then staging more (AC-4).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `batch_too_large` status 413 vs 422 undecided (`problem_details.md` open point 2); typed result unchanged | E-21/c02 | open | Low |
| Zero-byte files use the staged path with `block_count = 0` (`canonical-names.md` default); the 13 spec does not mention them | E-23 | accepted-risk | Low |
