# Worker suite: a second supervisor pair on the test stack; AT-09-2 and PT-09-2

**Task**: AZ-4391_worker_second_pair_scaling
**Name**: Second supervisor pair, AT-09-2 and PT-09-2
**Description**: A second supervisor/decoder pair on the Compose test stack, and the two worker cases that need it: kill-and-recover with two supervisors (AT-09-2) and two-supervisor scaling (PT-09-2)
**Complexity**: 5 points
**Dependencies**: AZ-3878_worker_integration_suite (H2, FD, the IT-09-14 kill legs and the `cpu-limit` action), AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl
**Component**: 09_worker_supervisor (test stack)
**Tracker**: AZ-4391
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3879 (E-20/c12)

## Problem

The test stack runs one supervisor (`worker`) with one decoder per lane (`tests/compose.test.yml`: `worker`,
`decoder-interactive`, `decoder-general`). AT-09-2 and PT-09-2 (`_docs/02_document/components/09_worker_supervisor/tests.md`)
need two supervisor/decoder pairs on the same queue. e2e-ctl knows only the first pair's services
(`tests/tools/e2e-ctl/ctl_actions.py` `INSPECTABLE`, `SCRATCH_LANES`, `LOG_SERVICES`), and the fixtures have one
transcode clip of each kind, not 20 distinct ones.

## Outcome

- A Compose profile (`pair2`, off by default; `up` of `core` or `h2` never starts it) adds `worker-b`,
  `decoder-interactive-b`, `decoder-general-b` with their own socket and scratch volumes, the same hardening, limits and
  H2 settings as the first pair, and FD services for its lanes.
- e2e-ctl: the second pair's services in the allow-lists of `process-kill`, `inspect` (state, scratch listing,
  processes, container facts) and logs; each new argument checked and refused like the existing ones, with host unit tests.
- `gen_media.py` makes 20 distinct non-compliant clips that each fit one small upload (2 MiB) and each need a transcode.
- AT-09-2 and PT-09-2 on harness H2.

## Acceptance Criteria

**AC-1: Kill-and-recover with two supervisors (AT-09-2, AC-71)**
Given both pairs running and 20 transcode jobs from the clip set
When one supervisor, picked at random, is killed during decode (its decoder's ffmpeg in the process list) and restarted
Then the other keeps claiming meanwhile; every job ends `succeeded` or `dead` with a hostile-class code; exactly one
derived blob exists per `(account, content_hash, kind, params_version)`; the four scratch volumes are empty

**AC-2: Two supervisors scale (PT-09-2, AC-11)**
Given 400 jobs and FD answering each in 200 ms
When they run with one supervisor and then with two on the same queue
Then duplicate claims are 0 and every job closes (functional, runs on any host); the throughput ratio is at least 1.7
(target 1.9), recorded with the host line, and counts only on REF-CPU4: elsewhere the ratio assertion is
`NOT-RUN(HW-REF)` and a row is added to `_docs/_process_leftovers/2026-11-14_ref-srv-timing-not-run.md`

**AC-3: The second pair is opt-in and controllable**
Given the stack started without the `pair2` profile
When the existing suites run
Then nothing changes; with the profile, each new ctl argument does what it says and refused ones exit 2 without side effects

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | two pairs, 20 clips | AT-09-2 | as `tests.md` | NFT-RES-36, AC-71 |
| AC-2 | 400 jobs, FD 200 ms | PT-09-2 | 0 duplicates; ratio on REF-CPU4 only | NFT-PERF-17 |
| AC-3 | `core`/`h2` without `pair2` | existing worker suite | unchanged | — |

## Constraints

- Harness H2; FD only for PT-09-2 (no media outcome), the real decoders for AT-09-2.
- AT-09-2 is the component acceptance with two supervisor pairs; AZ-3661 owns the end-to-end AC-71 with the derived
  janitor; both stay.
- A timing result from a host other than REF-CPU4 is never reported as a pass (`environment.md` § Where each tier runs).
