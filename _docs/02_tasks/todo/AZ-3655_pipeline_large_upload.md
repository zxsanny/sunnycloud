# Large-file upload pipeline (F4)

**Task**: AZ-3655_pipeline_large_upload
**Name**: Large-file upload pipeline
**Description**: Upload sessions, block lanes and the commit drive a file above 2 MiB from the HTTP routes through transfer, blobstore, quota, tree and feed, end to end against the real components
**Complexity**: 5 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3801_crate_skeleton_accept_loop, AZ-3803_body_limits_timeouts, AZ-3829_tree_transfer_feed_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3734_transfer_start_upload, AZ-3735_transfer_put_block, AZ-3736_transfer_resume_abort, AZ-3738_transfer_commit_upload, AZ-3756_transfer_janitors_reservations, AZ-3709_tree_publish_core, AZ-3711_tree_quota_gc_deltas, AZ-3712_tree_blob_ownership_check, AZ-3750_quota_ledger, AZ-3725_master_keyring, AZ-3726_blob_writer, AZ-3727_group_commit_barrier, AZ-3729_blob_registry_gc_queue, AZ-3743_orphan_sweeper, AZ-3775_feed_stamp, AZ-3777_feed_long_poll, AZ-3690_account_admin_and_guard, AZ-3841_job_planner_enqueue_effect, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3655
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p03

## Problem

A large upload touches 11, 03, 04, 07a, 02, 01 and 05, plus two janitors run by 12's scheduler. 03 receives `TreeCommit` and its commit effects through ports that only the composition root binds, and 02 receives the account guard, quota ledger and feed stamp the same way. Each component tests against recording doubles; no planned task proves that a real upload through the real routes publishes a revision, advances the feed once and leaves nothing behind after a crash.

## Outcome

- A file above 2 MiB uploaded through the real `serve` process over up to four lanes is committed as a revision, its quota is accounted, and the feed counter advances last in the commit.
- Every component operation of flow F4 has a production caller, and the session reaper and orphan sweep run from the scheduler.
- At least one integration test runs F4, including a server kill, through the real binaries over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F4 steps 1-6):
  1. Start route → `UploadService::start_upload` (`StartUpload` → `Started{upload_id, need_blocks}` or `NothingToSend`) → name rules, `TreeReader::get_node`, `QuotaLedger::reserve`, `BlobWriter::create` and the `uploading` blob row with a wrapped data key (`MasterKeyring`).
  2. Block route x 4 lanes → `UploadService::put_block` → BLAKE3 check before write → `BlobWriter::write_block` → group-commit barrier → 204 → bitmap bit.
  3. Session route → `UploadService::get_upload` for resume.
  4. Commit route → `CommitService::commit_upload` (blocklist check, hash roll-up, head sniff) → `TreeCommit::publish`: `AccountGuard::ensure_active` first, F8 checks, revision insert, blob flip to committed, `GcQueue` for a replaced blob, `QuotaLedger::adjust_used` and `release_reservation`, the injected `CommitEffect`s (03 blob effect, 08 media effect), `FeedStamp::stamp` last → COMMIT → `FeedNotifier::notify`.
  5. Scheduler (12) → `SessionReaper::run_cycle` (03) and `OrphanSweeper::run_cycle` (04).
- The composition-root bindings these seams need (the effects list given to 03, the ports given to 02) where E-22/c03 leaves them unproven.
- The end-to-end integration test of the steps above.

### Excluded
- Small files and batch commits (E-22/p04), conflict and delete semantics (E-22/p07), media job processing (E-22/p08, p09), client upload executors (E-23, E-26, E-24, E-27).

## Acceptance Criteria

AC-1..AC-6 are local to this task. AC-68 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Large file round trip**
Given a signed-in device and a 64 MiB file
When it starts a session, sends blocks over 4 lanes and commits through the real routes
Then the revision is current, the account feed counter advanced by exactly one, a held long-poll for the account wakes, and a download of the file has the same BLAKE3 hash

**AC-2: Resume sends only missing blocks**
Given a session with 10 of 32 blocks acknowledged and the connection dropped
When the client reads the session and resends
Then only the missing blocks are sent, the commit succeeds, and the stored file contains no zero-filled block

**AC-3: Typed refusals reach the client**
Given an account near its quota, a block with a wrong hash, a commit before all blocks arrived and an invalid name
When each request is sent
Then the client gets 507 `quota_exceeded` with nothing reserved, 422 with the block not stored, 409 with `need_blocks`, and a 4xx with the name absent from the listing

**AC-4: Guard and effects run inside the commit**
Given an account in state `deleting` and, separately, an active account committing a JPEG
When each commit is sent
Then the first is refused as account not active, and the second runs the blob effect and the media effect in the same transaction (a job row exists after commit, none after a rolled-back commit) with the feed stamp last

**AC-5: Janitors run from the scheduler**
Given an open session left idle past its TTL (test clock)
When the scheduler's session-reaper cycle runs
Then the session row is gone and its reserved bytes are released

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `UploadService::{start_upload, put_block, get_upload, abort_upload}`, `CommitService::commit_upload`, `TreeCommit::publish`, `AccountGuard::ensure_active`, `QuotaLedger::{reserve, adjust_used, release_reservation}`, `BlobWriter::{create, write_block, write_complete}`, `FeedStamp::stamp`, `FeedNotifier::notify`, and scheduler registrations of `SessionReaper` and `OrphanSweeper`

### Interaction criteria (Step 1.8)

**AC-68: Crash between blob write and commit leaves no dangling revision** (R1)
Given an upload whose blocks are all acknowledged
When the real server process is killed with SIGKILL before the commit (and in a second run while the commit request is in flight), restarted, and the test clock is moved past the session TTL and the 48 h orphan age while the scheduler cycles run
Then no committed revision references a missing blob, the orphan file is removed by the orphan sweep, and the session's reserved quota is released

## Non-Functional Requirements

**Performance**
- On REF-SRV a single 1 GiB upload reaches at least 90% of 1 Gbps (AC-2); measured in E-31/c42, not in this task's CI test.

**Reliability**
- A block is acknowledged only after its group sync (AC-38).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named operation and janitor registration has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in device, 64 MiB file, held long-poll | Start, 4 lanes, commit, download | Revision current, counter +1, poll woken, hash equal | |
| AC-2 | 10 of 32 blocks acked, connection dropped | Resume | Only missing blocks sent, no zero blocks | AC-3 |
| AC-3 | Near-quota account, bad hash, early commit, bad name | Each request | 507, 422, 409 `need_blocks`, 4xx | |
| AC-4 | Account `deleting`; active account with JPEG | Commit | Refused; job row only after a successful commit | |
| AC-5 | Idle session, test clock | Scheduler cycle | Session gone, reservation released | |
| AC-68 | All blocks acked | SIGKILL before and during commit, restart, clock past TTL and 48 h | No dangling revision, orphan swept, quota released | NFT-RES-34 |

## Constraints

- Owner: component 12 (`src/server/`): composition-root bindings and scheduler registrations for this flow, and its end-to-end tests among 12's integration tests. No SQL, blob I/O or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume over HTTP; no sunnycloud component is faked or bypassed. The kill uses the real process, not a fault hook inside a component.
- The component-level crash cases stay in E-14/c12; AC-68 here checks the cross-component outcome after a real restart.
- Downstream: E-31/c23 (FT-P-01, FT-P-03, FT-P-05, FT-P-06), E-31/c31 (FT-N-01, FT-N-03), E-31/c39 and E-31/c41 (NFT-RES-34) should depend on this task.

## Risks & Mitigation

**Risk 1: Effects list wired in the wrong order**
- *Risk*: the media effect or blob effect is missing from the list 12 hands to 03, so commits pass but jobs or blob flips never happen.
- *Mitigation*: AC-4 checks the job row and the committed blob state after a real commit.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1: worst-case resend (16 MiB) vs AC-3's 10 MiB bound is open (E-14/c01, E-23/c03) | Owner via E-14 spike | open | Medium |
| 409 for `need_blocks` decided 2026-11-07 (E-21/c02, G-11-1) | E-21 | resolved | Low |
