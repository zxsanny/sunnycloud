# Worker suite: bulk and large-input load, PT-09-1 and PT-09-3

**Task**: AZ-4392_worker_bulk_large_input_load
**Name**: Bulk and large-input load, PT-09-1 and PT-09-3
**Description**: The supervisor's overhead on 2,000 photo jobs and its memory on a 1 GiB input, with the consumer's block upload, the DS-25 generator and a supervisor resource sample they need
**Complexity**: 3 points
**Dependencies**: AZ-3878_worker_integration_suite (H2, FD), AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl
**Component**: 09_worker_supervisor (test stack)
**Tracker**: AZ-4392
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3879 (E-20/c12)

## Problem

PT-09-1 and PT-09-3 (`_docs/02_document/components/09_worker_supervisor/tests.md`) need inputs the consumer cannot make
today: its upload helper is one block of at most 2 MiB (`tests/consumer/stackkit.py` `Drive.upload`), so a DS-03-size
photo set and DS-25 (1 GiB) do not fit; there is no DS-25 generator; and nothing samples the supervisor's RSS and CPU over
a run (IT-09-13 reads one RSS value from `/metrics`).

## Outcome

- The consumer uploads a file of any size through the uploads routes (`/api/v1/uploads`, `.../blocks/{index}`, commit),
  BLAKE3 per block as the API checks it, streamed from disk.
- A DS-25 generator (1 GiB dense, seeded, with its manifest hash).
- A supervisor sample over a run: peak RSS and CPU share (e2e-ctl `resource-sample` or the worker's `/metrics`).
- PT-09-1 and PT-09-3 on harness H2.

## Acceptance Criteria

**AC-1: Overhead (PT-09-1, AC-10)**
Given 2,000 `stills` jobs of DS-03 size and FD answering each render in 50 ms, one supervisor
When they run
Then every job closes with no duplicate claim (functional, runs on any host); supervisor time per job above FD time
<= 150 ms (target 40), jobs/s >= 3 (target 6), RSS <= 128 MiB (target 64), CPU < 50% of one core are measured and
recorded with the host line, and count only on REF-SRV/REF-CPU4: elsewhere `NOT-RUN(HW-REF)`, with a row in
`_docs/_process_leftovers/2026-11-14_ref-srv-timing-not-run.md`

**AC-2: Streaming memory (PT-09-3, AC-72)**
Given DS-25 committed as a video input with the default `worker_max_input_video`
When its job runs
Then the input hash the decoder sees (FD record) equals the manifest, the supervisor is not restarted and its peak RSS
stays far below the input size (functional: no buffering); the bound <= 128 MiB (target 64) counts only on REF-SRV,
`NOT-RUN(HW-REF)` elsewhere

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 2,000 jobs, FD 50 ms | PT-09-1 | all close; bounds on REF only | NFT-PERF-14 |
| AC-2 | DS-25 | PT-09-3 | hash equal; no buffering; bound on REF only | FT-N-43 |

## Constraints

- Harness H2; FD only (no media outcome is claimed).
- A timing or resource result from a host other than REF-SRV/REF-CPU4 is never reported as a pass
  (`environment.md` § Where each tier runs).
