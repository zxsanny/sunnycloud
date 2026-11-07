# Group-commit fdatasync barrier

**Task**: AZ-3727_group_commit_barrier
**Name**: Group-commit barrier
**Description**: The in-process leader/follower barrier that syncs every dirty blob file of a group with `fdatasync` and only then releases the waiters, with a test-only fault hook around the sync
**Complexity**: 5 points
**Dependencies**: AZ-3723_blobstore_skeleton; notes: E-31/c06 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 04 blobstore
**Tracker**: AZ-3727
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c07

## Problem

A block may be acknowledged only after it is flushed to disk (AC-38, RESTRICT-09). One `fdatasync` per block would cap throughput; a timed batch (for example 1 s) would add latency to every block. The description asks for a leader/follower group commit that amortises syncs across lanes and sessions with no artificial wait (§5).

## Outcome

- Writers register the file they wrote; the first waiter starts a flush at once (after the optional `blob_sync_coalesce_delay`, default 0 ms), syncs every dirty file in the group, then releases all waiters of that group; writers arriving during a flush join the next one.
- `await_durable` returns `Ok` only after the flush that covers the write; when the sync fails every waiter of that group gets `Io` and later groups proceed once the fault clears.
- A test-only hook wraps the sync function so tests can block it, fail it and count flushes; it is not reachable in release builds.

## Scope

### Included
- The barrier, the `DurableToken` and `await_durable` semantics, the coalesce delay setting, the fault hook.

### Excluded
- `write_block` itself (E-10/c06, which depends on this task), the `block_fsync_duration_seconds` metric (E-10/c15), PT-04-3 load test (E-10/c14).

## Acceptance Criteria

**AC-1: No acknowledgement before the sync**
Given the sync function blocked for 300 ms by the hook
When several writes register and await
Then no `await_durable` returns before 300 ms

**AC-2: A failed sync reaches every waiter**
Given the hook makes one group's sync fail
When the waiters of that group and of the next group await
Then every waiter of the failed group gets `Io`, none gets `Ok`, and the next group returns `Ok` after the fault clears

**AC-3: Syncs are coalesced**
Given 100 concurrent writes spread over 10 files
When each awaits durability
Then all 100 return `Ok`, the hook counts at most 25 group flushes, and every file was synced after its last registered write

**AC-4: No artificial wait**
Given an idle barrier
When a single write registers and awaits
Then it returns within 20 ms

**AC-5: Hook absent from release builds**
Given a release build
When its public API and features are inspected
Then the fault hook cannot be enabled or reached

## Non-Functional Requirements

**Performance**
- `await_durable` p95 at most 100 ms with 400 concurrent writers, failure 250 ms; aggregate at least 112 MB/s (PT-04-3, E-10/c14).

**Reliability**
- Acknowledged-but-unsynced blocks after a simulated sync failure: 0 (AC-38).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Leader/follower with a blocked sync | Waiters pending until the sync ends |
| AC-2 | Fault injection on one group | `Io` to all its waiters; next group `Ok` |
| AC-3 | Group formation under concurrent arrivals | Late arrivals join the next group |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Hook blocks sync 300 ms | IT-04-5 (a) | Every wait >= 300 ms | AC-38 |
| AC-2 | Hook fails one sync | IT-04-5 (b) | `Io` to the whole group; recovery | NFT-RES-03 |
| AC-3 | 100 writes, 10 files, XFS loop volume | IT-04-5 (c) | <= 25 flushes; every file synced after its last write | |
| AC-4 | Idle barrier | IT-04-5 uncontended case | Ack <= 20 ms | |

## Runtime Completeness

- **Capability**: `fdatasync` group commit on the blob volume.
- **Production code that must exist**: a real `fdatasync` on every dirty file of the group before any waiter is released.
- **Allowed external stubs**: the test hook may block, fail or count the real sync; it never replaces it in a release build.
- **Unacceptable substitutes**: a no-op or skipped sync, a timer-based acknowledgement, acknowledging on page-cache write, a release-build switch that disables syncing.

## Constraints

- One API process in v1.0 (ADR-002), so the barrier is in-process. No database transaction is held across a sync.
- The XFS timing row needs the blob volume tool of E-31/c06 and a privileged runner; without them it reports `NOT-RUN(C-priv)`.
- Findings of E-10/c02 (XFS vs ext4, coalesce delay) are applied if that spike has finished; the default stays 0 ms otherwise.

## Risks & Mitigation

**Risk 1: Slow disk stalls every writer**
- *Risk*: one long sync holds all waiters (ext4 journal).
- *Mitigation*: XFS default; `block_fsync_duration_seconds` alert (E-10/c15); gate S14 (E-10/c02).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |

## Contract

This task implements the `await_durable` part of the contract at `_docs/02_document/contracts/04_blobstore/blob-writer.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
