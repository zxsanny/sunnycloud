# Group commit on XFS versus ext4 under 400 writers (spike, gate S14)

**Task**: AZ-3722_group_commit_fs_spike
**Name**: Group-commit filesystem spike
**Description**: Measure the cost of batched fdatasync for 400 concurrent 2 MiB writers on XFS and on ext4 and record whether XFS stays the default and whether the zero coalesce delay holds
**Complexity**: 2 points
**Dependencies**: None
**Component**: 04 blobstore
**Tracker**: AZ-3722
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c02

## Problem

A block is acknowledged only after a group `fdatasync` (AC-38). With many writers the sync cost decides ack latency and, through it, sync latency (AC-5) and throughput (AC-2). ext4's journal can stall group syncs; XFS is the documented default and ext4 informational (gate S14, `00_research/06_component_fit_matrix.md`). There is no measurement, and the reference server REF-SRV is not provisioned (R03), so the risk should be measured early, before E-10/c07 builds the barrier.

## Outcome

- Numbers for both filesystems on one host: ack wait p50, p95 and p99, number of group flushes per second, and aggregate MB/s, for 400 writers of 2 MiB blocks over 3 minutes, using the leader/follower group-sync pattern of the component description (§5).
- A written conclusion recorded in the blobstore component description (Risks row "Group commit stalls on a slow disk"): XFS default confirmed or not, the operator caveat for ext4, and whether `blob_sync_coalesce_delay` stays 0 ms.
- A status line for gate S14 that E-31/c02 (gate map) can cite; numbers from a non-reference host are marked informational.

## Scope

### Included
- A throwaway measurement probe that mimics the barrier (pwrite of 2 MiB, then one group `fdatasync` per dirty file, waiters released after the sync).
- Loop-mounted XFS and ext4 images of equal size on a privileged runner, created by hand for this spike.
- Recording results and the conclusion.

### Excluded
- The production barrier (E-10/c07) and PT-04-3 on the real barrier (E-10/c14).
- AC-5 end-to-end sync latency (NFT-PERF-09, E-31/c43).

## Acceptance Criteria

**AC-1: Both filesystems measured**
Given loop-mounted XFS and ext4 images of equal size on one host
When 400 writers each write 2 MiB blocks and wait for their group sync for 3 minutes on each filesystem
Then ack wait p50, p95 and p99, flushes per second and aggregate MB/s are recorded per filesystem together with the host's CPU, disk and kernel

**AC-2: Compared with the targets**
Given the PT-04-3 targets (p95 at most 100 ms, failure at 250 ms; aggregate at least 112 MB/s)
When the results are compared
Then the record states for each filesystem whether it meets them, and runs not on REF-SRV are labelled informational (`NOT-RUN(HW-REF)` for the gate itself)

**AC-3: Conclusion recorded**
Given the numbers
When the conclusion is written
Then the blobstore description's Risks row states whether XFS stays the default, the ext4 caveat for the operator guide, and whether the coalesce delay default stays 0 ms, with the measured figures

**AC-4: Probe stays out of production**
Given the measurement probe
When the spike ends
Then no probe code is merged into the blobstore crate; E-10/c07 takes over only the findings

## Non-Functional Requirements

**Performance**
- Targets compared: PT-04-3 (p95 <= 100 ms, failure 250 ms; aggregate >= 112 MB/s).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (measurement task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | XFS and ext4 images, 400 writers, 3 min | Probe run per filesystem | Latency percentiles, flushes/s, MB/s recorded | PT-04-3 scenario |
| AC-2 | Results | Comparison with targets | Pass/fail per filesystem; informational label off REF-SRV | PT-04-3 |
| AC-3 | Results | Document review | Conclusion and figures in the Risks row | gate S14 |

## Constraints

- Needs a privileged runner for loop mounts; without one the spike stops and reports, it does not substitute tmpfs or a different filesystem.

## Risks & Mitigation

**Risk 1: Non-reference hardware**
- *Risk*: numbers from a laptop or CI runner do not predict REF-SRV.
- *Mitigation*: label them informational; repeat on REF-SRV in E-10/c14 (PT-04-3) once it exists.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| REF-SRV not provisioned (R03); gate S14 numbers stay informational until it exists | Owner (hardware) | open | Medium |
