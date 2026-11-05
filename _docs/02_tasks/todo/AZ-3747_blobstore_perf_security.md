# Blobstore performance and security suites

**Task**: AZ-3747_blobstore_perf_security
**Name**: Blobstore perf and security tests
**Description**: The component performance suite PT-04-1..4 and security suite ST-04-1..8 against the real blobstore on loop-mounted XFS
**Complexity**: 5 points
**Dependencies**: AZ-3721_blob_rewrite_rule_spike, AZ-3725_master_keyring, AZ-3726_blob_writer, AZ-3727_group_commit_barrier, AZ-3728_blob_reader, AZ-3729_blob_registry_gc_queue, AZ-3745_scrubber, AZ-3746_blob_maintenance_ports; notes: E-31/c06 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 04 blobstore
**Tracker**: AZ-3747
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c14

## Problem

The blobstore's numbers (4 KiB range at most 5 ms p95, line-rate transfer with encryption, ack latency under 400 writers, scrub impact) and its security promises (stolen disk, wrong key, active attacks, traversal, cross-account, key hygiene) are release criteria (AC-2, AC-24, AC-38, AC-43, AC-59). They need one suite that runs against the real component, on the reference hardware where timing matters.

## Outcome

- PT-04-1..4 implemented with the targets and failure thresholds of the component `tests.md`; timing rows report `NOT-RUN(HW-REF)` off REF-SRV.
- ST-04-1..7 implemented; ST-04-8 implemented per the rule decided in E-10/c01.

## Scope

### Included
- Load drivers, measurements, scans and assertions for PT-04-1..4 and ST-04-1..8 at the port boundary.

### Excluded
- System scenarios (NFT-PERF-02, 03, 11; NFT-SEC-23..26) in E-31; metrics (E-10/c15).

## Acceptance Criteria

**AC-1: 4 KiB range reads (PT-04-1)**
Given 16 tasks doing 100,000 random 4 KiB reads of a 1 GiB blob, warm and cold
When the run finishes on REF-SRV
Then p95 is at most 2 ms warm and 4 ms cold, no run exceeds 5 ms p95, and the error rate is 0%

**AC-2: Single-stream throughput (PT-04-2)**
Given one 5 GiB DS-09 blob written by 4 lanes of 2 MiB blocks and read sequentially
When the run finishes on REF-SRV
Then write is at least 300 MB/s and read at least 500 MB/s, and neither is below 112 MB/s

**AC-3: Group commit under 400 writers (PT-04-3)**
Given 400 concurrent block-write and await loops of 2 MiB for 3 minutes on XFS
When the run finishes on REF-SRV
Then `await_durable` p95 is at most 100 ms (failure 250 ms), aggregate is at least 112 MB/s, and after a simulated sync failure 0 blocks are acknowledged but unsynced; the ext4 run is recorded as informational

**AC-4: Scrub does not degrade writes (PT-04-4)**
Given a 60 s baseline of 4-lane writes, then the same with a CLI-style scrub at 50 MiB/s over 20 GiB
When both finish
Then write throughput with scrub is at least 95% of baseline (failure below 90%) and the scrub rate stays at or below 55 MiB/s (failure at 60 MiB/s)

**AC-5: Stolen disk yields nothing (ST-04-1)**
Given files uploaded with the DS-09 marker and known strings such as `PASSWORD=hunter2`
When the whole loop image, the file tree, `.keycheck` and `.epoch` are scanned
Then 0 marker hits, 0 known-string hits, and the master key bytes are found nowhere on the volume

**AC-6: Wrong key and active attacks (ST-04-2, ST-04-3)**
Given another master key, a ring lacking an id, the IT-04-4 damage cases, a rolled-back chunk, a whole file moved to another blob id and B1's wrapped key used for B2
When each is loaded or opened and consumed
Then startup gives `KeyCheckMismatch`, opens give `KeyUnavailable` or `Corrupt`, and 0 bytes of plaintext are returned in all 8 + 3 cases

**AC-7: Traversal and id forgery (ST-04-4)**
Given ids `../../etc/passwd`, `00/../..`, a non-UUID and a UUID with uppercase or braces
When `open_for_write`, `open` and `discard` are called with them under a path-recording decorator
Then each fails with a typed error or `BlobMissing`, and every opened path starts with the blob root

**AC-8: Cross-account and roles (ST-04-5, ST-04-6)**
Given blobs and queue rows of accounts A and B
When B calls `get_ref(B, A.blob_id)`, submits A's `BlobRef`, counts pending GC, reads raw rows under B's scope and tries `ALTER TABLE blobs`
Then `get_ref` returns `None`, pending counts only B's blobs, B sees only B's rows, sweepers in system scope see all, and DDL is denied

**AC-9: Key hygiene (ST-04-7)**
Given every error path (wrong key, tamper, missing)
When logs, the `/metrics` text and the `Display` and `Debug` output of every `BlobError` and `KeyError` are scanned, and key types are checked at compile time
Then 0 matches of the 32 key bytes in hex or base64 are found, `Debug` of key types prints a redaction, and key types zeroize on drop

**AC-10: Block rewrite (ST-04-8)**
Given the rule decided in E-10/c01
When a caller writes block i with bytes A and then with bytes B
Then the outcome matches the rule, and two different plaintexts at one block index are never both readable as valid

## Non-Functional Requirements

**Performance**
- As AC-1 to AC-4; CPU at most 4 cores for AEAD; blocking pool 2 x cores.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-9 | Compile-time zeroize assertion for key types | Compiles only when key types zeroize on drop |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 1 GiB blob, 16 tasks | PT-04-1 | p95 <= 2/4 ms, never > 5 ms | NFT-PERF-11, AC-43 |
| AC-2 | DS-09 5 GiB | PT-04-2 | >= 300 / 500 MB/s | NFT-PERF-02, AC-2 |
| AC-3 | 400 writers, XFS and ext4 | PT-04-3 | p95 <= 100 ms; 0 unsynced acks | NFT-PERF-03, AC-38 |
| AC-4 | Baseline and scrub at 50 MiB/s | PT-04-4 | >= 95% baseline; <= 55 MiB/s | NFT-RES-LIM-05, AC-24 |
| AC-5 | Marker uploads | ST-04-1 | 0 hits | NFT-SEC-24 |
| AC-6 | Key and attack cases | ST-04-2, ST-04-3 | 0 plaintext bytes | FT-N-12, NFT-SEC-23, NFT-SEC-25 |
| AC-7 | Hostile ids | ST-04-4 | 0 paths outside the root | |
| AC-8 | Accounts A and B | ST-04-5, ST-04-6 | 0 foreign refs; DDL denied | NFT-SEC-13, NFT-SEC-46 |
| AC-9 | Error paths | ST-04-7 | 0 key-byte matches | NFT-SEC-38 |
| AC-10 | Rewrite case | ST-04-8 | Per the E-10/c01 rule | |

## Constraints

- Timing rows run on REF-SRV (env C-REF); elsewhere they report `NOT-RUN(HW-REF)`. Loop-volume rows need E-31/c06's volume tool and a privileged runner (`NOT-RUN(C-priv)` otherwise). A `NOT-RUN` row is reported, never counted as a pass.
- Real PostgreSQL 18 and the real component; no stand-in for the cipher, the disk or the sync.

## Risks & Mitigation

**Risk 1: No reference hardware (R03)**
- *Risk*: the timing targets cannot be confirmed before release.
- *Mitigation*: rows stay `NOT-RUN(HW-REF)` and visible in the release gate (E-31/c47) until REF-SRV exists.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| REF-SRV not provisioned (R03); PT-04-1..4 cannot pass until it is | Owner (hardware) | open | Medium |
| ST-04-8 waits for the owner's rewrite rule (E-10/c01) | Owner | open | Medium |
