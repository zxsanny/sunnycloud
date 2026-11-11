# Sync core performance and acceptance tests (PT-13-1, PT-13-2, PT-13-4, AT-13-1, AT-13-2)

**Task**: AZ-3972_sync_perf_acceptance
**Name**: Performance and acceptance tests
**Description**: Measure the client's large-transfer throughput, mobile-profile memory and small-file batch time on the reference hardware, and run the two acceptance tests: a killed 5 GiB upload that resumes cheaply, and corruption and version mismatch handled clearly
**Complexity**: 3 points
**Dependencies**: AZ-3951_sync_s1_client_spike, AZ-3953_sync_resend_bound_spike, AZ-3959_http_sync_client_connect, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3964_verified_download, AZ-3971_sync_real_server_suite; notes: E-31/c15 (test epic E-31, decomposed at greenfield Step 9); E-31/c18 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9); E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 13_sync_core
**Tracker**: AZ-3972
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c18b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — operations under test.
- `_docs/02_document/tests/performance-tests.md` — NFT-PERF-04, NFT-PERF-05, NFT-PERF-07.
- `_docs/02_document/tests/spec-gaps.md` — G-03-1 (AT-13-1 bound), G1/G11 (reference hardware).

## Problem

The component acceptance of 13 (§10) promises: a 5 GiB upload killed at random points re-sends at most 10 MiB per resume; 10,000 small files commit in at most 60 s at 20 ms RTT with no duplicates after a retried batch; a flipped bit is rejected and re-sent; a corrupt download delivers nothing as valid; an unsupported server gives a clear message; the desktop large transfer reaches at least 90% of 1 Gbps. These need the reference hardware and the finished executor; this is the second half of the split row E-23/c18 (the first is E-23/c18a). PT-13-3 (planner cost) is already AC-5 of E-23/c05 because it needs no server.

## Outcome

- PT-13-1, PT-13-2 and PT-13-4 measured with results recorded, or `NOT-RUN(HW-REF)` where the hardware is missing.
- AT-13-1 and AT-13-2 pass, with AT-13-1's bound taken from E-23/c03.

## Scope

### Included
- PT-13-1: 1 client, 4 lanes, DS-09 up and down, encryption at rest on, `iperf3` calibration first, windows from E-23/c02.
- PT-13-2: mobile configuration, DS-09 plus 50,000 DS-21 files, RSS every 1 s.
- PT-13-4: DS-01 (10,000 files), `shape-wan`, parallel `stage_small`, batches of `batch_max_entries`, one retried batch.
- AT-13-1: 10 seeded drops of a 5 GiB upload, re-send per resume from the proxy log.
- AT-13-2: flipped bit in transit, corrupt download, server with `min_client_version` above the client.

### Excluded
- The integration legs (E-23/c18a); system scenarios NFT-PERF-* and FT-* (E-31).

## Acceptance Criteria

**AC-1: Large transfer at 90% of 1 Gbps (AC-2)**
Given REF-SRV and REF-LOAD, an `iperf3` calibration and DS-09 with encryption at rest on
When the client uploads and downloads with 4 lanes
Then goodput is at least 112 MB/s and at least 90% of calibrated goodput, 0 blocks fail, client CPU stays below one full core and RSS at or below 150 MB (failure below 100 MB/s)

**AC-2: Mobile-profile memory (AC-52)**
Given the mobile configuration, DS-09 and 50,000 DS-21 files
When they are uploaded with RSS sampled every 1 s
Then peak RSS is at most 100 MB (failure above 150 MB) and no whole-file buffer is held (at most 4 x 2 MiB block buffers)

**AC-3: 10,000 small files (AC-40)**
Given DS-01 at 20 ms RTT (`shape-wan`) on the reference hardware
When the client stages and batch-commits all files, with one batch retried
Then the total time is at most 45 s (failure above 60 s), 0 entries fail, 0 duplicates exist after the retried batch, and RSS stays at or below 150 MB

**AC-4: Killed 5 GiB upload resumes cheaply (AC-3)**
Given DS-09 and `shape-flaky` dropping the connection at 10 seeded random offsets
When the upload resumes after each drop
Then block PUT bytes per resume (proxy log) stay at or below the bound recorded by E-23/c03 (16 MiB, AC-3 as changed 2026-11-11 by G-03-1), hashing never restarts from zero, and the server hash equals the local BLAKE3

**AC-5: Corruption and version mismatch (AC-7, AC-64)**
Given the fake and the real server
When one bit of an upload block is flipped in transit, one block of a download is corrupted, and the client connects to a server whose `min_client_version` is above it
Then the server answers 422, the block is re-sent once and the file commits with the right hash; the download fails with `HashMismatch` and nothing is delivered as valid; `connect` returns `UpgradeRequired` with the server text and no retry

**AC-6: Hardware gate**
Given REF-SRV or REF-LOAD is not available
When AC-1 or AC-3 is due
Then it is recorded as `NOT-RUN(HW-REF)` in `_docs/_process_leftovers/` and no workstation result is reported as the outcome

## Non-Functional Requirements

**Performance**
- Targets as stated per AC (13 tests PT-13-1, PT-13-2, PT-13-4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (performance and acceptance tests) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | C-REF, DS-09 | Throughput up and down | >= 112 MB/s, 0 failed blocks | PT-13-1, NFT-PERF-04 |
| AC-2 | Mobile config, DS-09 + DS-21 | RSS | <= 100 MB target, > 150 MB fail | PT-13-2, NFT-RES-LIM-03 |
| AC-3 | C-REF, DS-01, `shape-wan` | Batch time | <= 45 s target, > 60 s fail | PT-13-4, NFT-PERF-07, FT-P-04 |
| AC-4 | DS-09, `shape-flaky`, 10 drops | Re-send per resume | <= bound from E-23/c03 | AT-13-1, FT-P-01, NFT-PERF-05, NFT-RES-01 |
| AC-5 | Fake and real server | Bit flip, corrupt download, old client | 422 then re-send; `HashMismatch`; `UpgradeRequired` | AT-13-2, FT-N-01, FT-N-02, FT-N-27 |
| AC-6 | No reference hardware | Run attempt | `NOT-RUN(HW-REF)` | |

## Constraints

- C-REF timings are valid only on REF-SRV/REF-LOAD (13 tests tiers).

## Risks & Mitigation

**Risk 1: AT-13-1 fails legitimately while G-03-1 is open**
- *Risk*: the 16 MiB worst case exceeds 10 MiB.
- *Mitigation*: AC-4 takes its bound from E-23/c03 and records a legitimate failure explicitly.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1 worst-case re-send 16 MiB vs AC-3's 10 MiB (AC-4) | Owner (via E-14/c01, E-23/c03) | resolved 2026-11-11 (AZ-3731: AC-3 is 16 MiB; commit answers in commit_service.md) | Medium |
| Reference hardware not yet named (G1, G11, R03) | Owner | open | Medium |
| PT-13-2 runs the mobile configuration on a container host; how it maps to phone memory (RSS vs PSS, resource-limit G2) is not stated | Owner / E-31 | open | Low |
