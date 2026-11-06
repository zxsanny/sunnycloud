# Sync core hash_file: resumable, throttled block hashing of a host file

**Task**: AZ-3961_sync_hash_file
**Name**: Resumable file hashing
**Description**: The `hash_file` operation that reads a host `FileSource` block by block, produces the blocklist and file hash, resumes from a saved hash state, throttles to `sync.hash_cpu_share` and detects a file that changed while being hashed
**Complexity**: 2 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3961
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c09a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `hash_file` (this task contributes it).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `FileSource`, `ProgressSink`, `CheckpointStore` (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — hashing wrappers, `ResumeHashState`.

## Problem

Every large upload starts with hashing the whole file (13 §3: "hash (resumable per block) -> start_upload"). For a 50 GiB file that is minutes of work; if a kill or a drop forces hashing from zero each time, AC-3's resume promise is meaningless (AT-13-1 requires no re-hash from zero) and mobile runs re-hash far more than 1% of assets (AC-52). On the desktop, initial hashing may use at most 25% of one core while the user is active (AC-15). The row "Large-upload executor" (E-23/c09) covers this step too; it is split out here because hashing plus the lane executor exceed 5 points, and hashing has its own inputs and tests.

## Outcome

- `hash_file(source, resume, progress) -> HashedFile{block_hashes, file_hash}` as in the `sync_api` contract.
- Hash state handed back every `sync.hash_checkpoint_blocks` (32) blocks so a later call resumes at the last saved block boundary.
- CPU use bounded by `sync.hash_cpu_share`; a source whose identity changes during hashing ends in `SourceChanged`.

## Scope

### Included
- Reading the source one block (2 MiB) at a time; one block buffer.
- Resume from `ResumeHashState`: only blocks after `blocks_done` are read.
- Checkpointing the hash state every 32 blocks through the host (mechanism recorded with E-23/c14, `host_traits.md` open point 2).
- Throttling to `sync.hash_cpu_share` of one core.
- Identity check (`size`, `mtime_ns`) at start and end; a change is `SourceChanged`.
- Progress reported through `ProgressSink`.

### Excluded
- Uploading and the identity re-check before commit (E-23/c09b).
- Desktop rules for when the user counts as active (14).

## Acceptance Criteria

**AC-1: Same hashes as the vectors (AC-7)**
Given the file cases of the vector file (0, 1, 2 MiB - 1, 2 MiB, 2 MiB + 1, 6 MiB)
When `hash_file` runs on each through a `FileSource`
Then the block hashes and file hash equal the vector values, and a 0-byte file gives the empty-rollup hash

**AC-2: Resume does not re-hash from zero (AC-3)**
Given a 100-block file hashed up to a saved state after block 64 and then interrupted
When `hash_file` is called again with that state
Then the source sees reads only for blocks 64 to 99, and the result equals a full hash of the file

**AC-3: State saved every 32 blocks**
Given `sync.hash_checkpoint_blocks = 32` and a 100-block file
When hashing runs to the end
Then the hash state is handed to the host after blocks 32, 64 and 96

**AC-4: Changed source detected**
Given a source whose `identity()` changes between the start and the end of hashing
When `hash_file` finishes reading
Then it returns `SourceChanged` and no `HashedFile`

**AC-5: CPU share honoured (AC-15)**
Given `sync.hash_cpu_share = 0.25` and a 1 GiB file on a workstation core
When `hash_file` runs
Then process CPU time divided by wall time stays at or below 0.25 plus a 0.05 measurement margin over the run

**AC-6: Bounded memory (AC-52)**
Given a 5 GiB source
When it is hashed
Then the operation holds at most one 2 MiB block buffer plus hash state

## Non-Functional Requirements

**Performance**
- Unthrottled (`hash_cpu_share = 1.0`) hashing is limited by the source read rate, not by the operation.

**Reliability**
- An `Io` error from the source ends the call with `Io` and the last saved state stays valid.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Vector file cases through an in-memory source | Equal hashes |
| AC-2 | Resume from block 64 with a read-counting source | Reads 64..99 only; equal result |
| AC-3 | Checkpoint cadence | After 32, 64, 96 |
| AC-4 | Identity change mid-run | `SourceChanged` |
| AC-6 | Allocation sampling on a 5 GiB generated source | One block buffer |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-5 | 1 GiB file, share 0.25 | CPU over the run | <= 0.25 (+0.05 margin) | AC-15 |
| AC-2 | DS-09 partially hashed | Resume | No re-hash from zero | AT-13-1, NFT-RES-01 |

## Constraints

- Hashing comes from `sc_ids` through `sync-core-proto` (no second implementation).
- The throttle reads `sync.hash_cpu_share` from config; no hard-coded share.

## Risks & Mitigation

**Risk 1: Flaky CPU measurement on shared CI runners**
- *Risk*: AC-5 fails on a noisy runner.
- *Mitigation*: the 0.05 margin and a 1 GiB sample; the AC-15 system check stays with 14.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How the hash state reaches the host every 32 blocks (through `CheckpointStore::save` or another callback) is not stated (`host_traits.md` open point 2) | E-23 (with c14) | open | Low |
| AC-5's 0.05 measurement margin is proposed by this task; 13 gives none | E-23 | open | Low |
