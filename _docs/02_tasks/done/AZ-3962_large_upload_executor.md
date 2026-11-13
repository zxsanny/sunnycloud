# Large-upload executor: lanes, ack on 204, resume, lost-commit recovery

**Task**: AZ-3962_large_upload_executor
**Name**: Large-upload executor
**Description**: The `upload` operation that runs the planner over HTTP/2: start or resume a session with the stored `client_key`, send blocks on up to four lanes with their BLAKE3, count a block only on 204, commit, map conflict outcomes, and recover a lost commit response with the same key
**Complexity**: 5 points
**Dependencies**: AZ-3949_sync_nothing_to_send_spike, AZ-3957_upload_planner, AZ-3959_http_sync_client_connect, AZ-3961_sync_hash_file, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server, AZ-3731_transfer_spike_resend_stale_base
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3962
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c09b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `upload` (this task contributes it), wire table.
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `UploadPlanner`, `StartOutcome`, `CommitOutcome`.
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `FileSource`, `CheckpointStore`, `ProgressSink`.
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md` — session, block and commit behaviour (drafts; `NothingToSend` rule from E-14/c02, commit answers from E-14/c01).

## Problem

A 50 GiB upload over a flaky link must finish, resume after drops re-sending at most 16 MiB (AC-3, G-03-1), count a block only after the server made it durable (AC-38, AC-68 client share), resend a block whose bytes were flipped in transit (AC-7, FT-N-01), and finish in constant time (AC-4). A lost commit response must not create a duplicate or a conflict copy of the user's own file (AC-78 contribution, AC-46). The planner (E-23/c05) decides; nothing executes its actions over the network yet. The hashing step of the same flow is E-23/c09a (split from this row because both together exceed 5 points).

## Outcome

- `upload(UploadRequest, Arc<dyn CheckpointStore>, progress) -> CommitOutcome` as in the `sync_api` contract.
- Blocks on at most `min(4, negotiated lanes)` concurrent requests, each with its `X-Blake3`; a block is done only on 204.
- Resume after any interruption re-reads the session and sends only `need_blocks`; the stored `client_key` is reused on every retry and resume.
- A lost commit response recovered by `start_upload` with the same key, with the outcome rule recorded by E-23/c01.

## Scope

### Included
- `start_upload(client_key, content_hash, base_revision?)`; `NothingToSend{revision}` ends the upload with 0 block bytes.
- The checkpoint (`UploadCheckpoint`) saved before the first block and cleared after a final outcome; the stored key reused; a new key only for new content.
- The lane loop driving `UploadPlanner`: `SendBlock`, `Wait` (with jitter added here, `Retry-After` honoured), `RereadSession`, `RestartSession`, `Commit`, `Done`.
- Block results: 204 → `Acked`; 422 `block_hash_mismatch` → `HashRejected`; 409 → `BlockConflict`; 410 → `SessionGone`; 200, 202, 5xx, a reset or a timeout → `Transient`.
- Before commit: source identity re-check (`SourceChanged`); after `BlocklistMismatch`: re-hash (E-23/c09a) and restart with the same key.
- Commit outcomes `Published`, `ConflictCopy` (server-chosen name; the client never retries with a new name) and the typed errors of the contract.
- Lost commit response (network error or `upload_session_gone` at commit): `start_upload` again with the same key and hash.
- Progress reported for acknowledged blocks only.

### Excluded
- Hashing (E-23/c09a), small files (E-23/c10), the planner rules themselves (E-23/c05).
- The real-server legs of IT-13-3, IT-13-6 and IT-13-16 (E-23/c18a) and AT-13-1 (E-23/c18b).

## Acceptance Criteria

**AC-1: Ack only on 204 (AC-38, AC-68 client share)**
Given a scripted fake server answering a block PUT with 200 plus a body, 202, 500 after reading the body, and a reset after the full body
When the upload meets each answer
Then the block stays needed and is sent again, and `ProgressSink` counts only blocks answered 204

**AC-2: Flipped bit is re-sent (AC-7)**
Given a fake that answers 422 `block_hash_mismatch` once for block 3
When the upload runs
Then block 3 is sent again once and the commit succeeds with the client's file hash

**AC-3: Lanes and resume (AC-3)**
Given negotiated lanes 4 and a fake that drops the connection after 37 acked blocks of 100
When the upload resumes
Then at no time are more than 4 block requests in flight, the session is re-read, only the blocks in `need_blocks` are sent, and the same `client_key` is used

**AC-4: Stored key reused across retries (AC-78 contribution)**
Given a desktop upload retried 3 times with a `MemoryCheckpointStore`
When each attempt calls `start_upload`
Then all 3 attempts carry one UUIDv7 `client_key`, and a later upload of new content gets a new key

**AC-5: Nothing to send**
Given a fake answering `start_upload` with `NothingToSend{revision}`
When `upload` runs
Then it returns `NothingToSend{revision}` and sends 0 block bytes

**AC-6: Lost commit response, existing node (IT-13-16 A)**
Given an upload of a new revision of an existing node (base revision set) whose commit response is dropped once
When the client recovers
Then it calls `start_upload` with the same key and hash, the result is `NothingToSend{revision}`, and 0 block bytes are re-sent

**AC-7: Lost commit response, new file (IT-13-16 B)**
Given a brand-new file at `(parent, name, hash)` whose commit response is dropped once
When the client recovers
Then the result is the one recorded by E-23/c01 (`sync_api.md` § Lost-commit recovery): `NothingToSend{revision}`, 0 block bytes re-sent, 1 server revision, no conflict copy; the case where the name was taken at commit (IT-13-16 C) is asserted once E-14's committed-key answer (AZ-3734, AZ-3738) lands

**AC-8: Conflict outcomes (AC-8)**
Given a fake answering the commit with a conflict copy for a stale base revision, and with a server-assigned copy name for a new-file name collision
When `upload` runs for each
Then the results are `ConflictCopy{node, copy}` with the server's name, and for the collision exactly 1 commit request is sent (no client retry with another name)

**AC-9: Changed source restarts (FT-N-03)**
Given a source whose identity changes before commit, and a fake answering the commit with `blocklist_mismatch`
When the upload reaches the commit
Then the first ends in `SourceChanged` without a commit request, and the second re-hashes and restarts with the same `client_key`

## Non-Functional Requirements

**Performance**
- Between the last 204 and the commit request the client does no work that grows with file size other than the identity check (AC-4; the 2 s bound is measured in E-23/c18a, IT-13-3).

**Reliability**
- Memory: at most `min(4, lanes)` block buffers of 2 MiB plus hash state; no whole-file buffer (AC-52).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Status-to-result mapping for block PUTs | Only 204 is `Acked` |
| AC-4 | Key reuse through `MemoryCheckpointStore` across 3 attempts | One key (IT-13-17) |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake server (E-23/c17) | Non-204 answers | Re-sent; progress counts 204 only | IT-13-15, NFT-RES-03 |
| AC-2 | Fake with one 422 | Bit flip | Resent once; commit ok | FT-N-01, NFT-SEC-21, AT-13-2 step 1 |
| AC-3 | Fake drop at block 37 | Resume | <= 4 in flight; only needed blocks | IT-13-2, FT-P-01, NFT-RES-01 |
| AC-5 | Fake `NothingToSend` | Upload | 0 block bytes | IT-13-16 |
| AC-6 | Fake drops commit response | Recovery, existing node | `NothingToSend`, 0 bytes re-sent | IT-13-16 (A), AC-78 |
| AC-7 | Fake drops commit response | Recovery, new file | `NothingToSend`; 1 revision; 0 bytes | IT-13-16 (B) |
| AC-8 | Fake conflict answers | Stale base; name collision | `ConflictCopy`; 1 request | IT-13-6, FT-P-12 |
| AC-9 | Fake and changing source | Identity change; mismatch | `SourceChanged`; restart same key | FT-N-03 |

## Constraints

- The client never renames; conflict-copy names come from the server (ADR-005, `canonical-names.md`).
- Retry timing comes from `sync.retry_base`, `sync.retry_max`, `sync.max_attempts_per_block`; tests use short config values, never real sleeps.

## Risks & Mitigation

**Risk 1: Recovery creates a conflict copy of the user's own file**
- *Risk*: 03 does not match a new file after a lost response (13 risk 2).
- *Mitigation*: Recorded by AZ-3949 (2026-11-13); branch IT-13-16 (C) is asserted after AZ-3734/AZ-3738.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-13-16 (B) new-file recovery depends on E-14/c02 and E-23/c01 | E-14 / E-23 | resolved 2026-11-13 (AZ-3949: sync_api.md § Lost-commit recovery; IT-13-16 C by 03's committed-key answer, owner) | Low |
| G-03-3: commit answers for stale-base and taken-name commits and whether `EditedSince` occurs on commit (E-14/c01) decide AC-8's exact fields | Owner (via E-14/c01) | resolved 2026-11-11 (AZ-3731: AC-3 is 16 MiB; commit answers in commit_service.md) | Medium |
| G-03-1: worst-case re-send 16 MiB vs AC-3's 10 MiB; AC-3 here checks the lane bound (planner `lanes_max` = `effective_lanes(ServerInfo.max_lanes, UploadSession.lane_limit)`; `Limits.lanes` in `negotiate.rs` does not include `lane_limit`); the 16 MiB bound is asserted in AT-13-1 (AZ-3953) | Owner (via E-14/c01, E-23/c03) | resolved 2026-11-11 (AZ-3731: AC-3 is 16 MiB; commit answers in commit_service.md) | Medium |
