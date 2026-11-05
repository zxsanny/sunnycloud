# Decoder performance, security and acceptance tests PT-10-1..3, ST-10-1..4, AT-10-1..2

**Task**: AZ-3866_decoder_perf_security_acceptance
**Name**: Decoder perf, security and acceptance tests
**Description**: Measure photo and transcode speed and idle cost, prove the containers hold no credential, network or blob access, check argument hygiene, the sniff matrix and log disclosure, and run the two component acceptance scenarios
**Complexity**: 3 points
**Dependencies**: AZ-3865_decoder_integration_suite; notes: E-31/c01 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 10_decoder
**Tracker**: AZ-3866
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/components/10_decoder/tests.md` — PT-10-1..3, ST-10-1..4, AT-10-1..2.

## Problem

The decoder's speed budgets (AC-10, AC-11, AC-44) and its isolation (AC-25) must be shown on the real image under production limits and, for speed, on the reference hardware. The security tests guard the properties a reviewer relies on: no secret, no route, no blob mount, no shell, no leakage into logs.

## Outcome

- Measured PT-10-1..3 results against their targets and failure thresholds; PT-10-2 only on REF-CPU4.
- ST-10-1..4 and AT-10-1..2 pass on the production containers.

## Scope

### Included
- PT-10-1 photo latency, PT-10-2 transcode speed (REF-CPU4 only, else `NOT-RUN(HW-REF)`), PT-10-3 idle cost and `Alive` cadence.
- ST-10-1 no credentials, network or blob access; ST-10-2 argument and environment hygiene; ST-10-3 hostile-type matrix; ST-10-4 disclosure in logs.
- AT-10-1 corpus then normal service; AT-10-2 outputs and kill timing.

### Excluded
- The full 10-minute budgets and the 100-clip ratio (NFT-PERF-15, NFT-PERF-16, NFT-PERF-20, E-31).

## Acceptance Criteria

**AC-1: Photo latency (PT-10-1, AC-10)**
Given 100 JPEG then 20 HEIC of DS-03 on the interactive limits (2 CPU, 2 GiB)
When each is rendered at 1440 px then 320 px
Then JPEG pair p95 is at most 2 s (target 600 ms), HEIC pair p95 at most 3 s (target 1.5 s), probe p95 at most 100 ms (target 30 ms)

**AC-2: Transcode speed (PT-10-2, AC-11, AC-44)**
Given DS-05 base and DS-06 base on REF-CPU4
When each is transcoded one at a time
Then 1080p is at most 0.5 s per second of media, 4K HLG at most 2.0 s/s, and the DS-05 output/input ratio at most 35% (target 20%); off REF-CPU4 the result is `NOT-RUN(HW-REF)`

**AC-3: Idle and alive cost (PT-10-3, AC-45)**
Given a runner idle for 10 minutes and then a 60 s transcode, and then 100 jobs
When CPU, events and memory are sampled
Then idle CPU is under 2% (target 0.5%), `Alive` intervals are within 6 s (target 5 s +/- 0.5 s), and memory growth over 100 jobs is under 100 MiB (target 20 MiB)

**AC-4: No credentials, network or blob access (ST-10-1, AC-25)**
Given both decoder containers
When routes, resolver, mounts, environment, `CapEff` and cgroup limits are inspected, and a stub child tries `api:8081`, `db:5432`, an external IP and the blob path
Then only loopback exists, every connect fails, no blob path or secret is present, and limits are 2 CPU / 2 GiB and 4 CPU / 4 GiB with `pids_limit` set

**AC-5: Argument hygiene (ST-10-2)**
Given photo and video jobs with hostile `job_dir` names and a `Plan` with a free-form field and `max_edge` -1, 0 and 100000
When the stub argv and environment logs are compared with the golden files
Then they match, no shell is created, the environment is exactly the allowlist per profile, and the malformed requests are `ProtocolViolation`

**AC-6: Hostile-type matrix (ST-10-3)**
Given a JPEG+HTML polyglot, a GIF89a/HTML file, an MP4 with a `heic` brand, an EBML file with HLS text and three controls
When each runs as a job
Then the profile chosen for each of the 7 inputs matches the sniff table, and the image lists no SVG, PDF or Magick loader

**AC-7: No disclosure in logs (ST-10-4)**
Given a stub writing 1 MiB of stderr with a fake secret and ANSI and newline sequences, then killed
When the runner log is read
Then the tail is at most 2,048 bytes, sanitised, and carries `job_id` but no user content paths

**AC-8: Acceptance (AT-10-1, AT-10-2)**
Given both decoders, DS-16, DS-03, DS-08 and DS-35
When every DS-16 item runs, then the controls; and rotated/VFR clips are transcoded, the moov-at-end clip remuxed and ffmpeg stopped mid-transcode
Then every DS-16 item fails typed with no exposure and the controls succeed with no extra process and empty scratch; orientation is right with drift at most 40 ms, packets are identical with the index first, and `Stalled` arrives within 90 s of the last progress line

## Non-Functional Requirements

**Performance**
- The thresholds above; speed results count only on REF-CPU4 (`tests/environment.md`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (test task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-03, 2 CPU / 2 GiB | PT-10-1 | Within thresholds | NFT-PERF-13 |
| AC-2 | DS-05/06 base, REF-CPU4 | PT-10-2 | Within thresholds or NOT-RUN | NFT-PERF-15, NFT-PERF-16, NFT-PERF-20 |
| AC-3 | Idle runner, 100 jobs | PT-10-3 | Within thresholds | NFT-PERF-21 |
| AC-4 | Both containers | ST-10-1 | No route, secret, mount | NFT-SEC-29, AC-25 |
| AC-5 | Stub logs, hostile requests | ST-10-2 | Golden match; violations | AC-25 |
| AC-6 | Polyglots | ST-10-3 | Sniff table holds | AC-25 |
| AC-7 | Noisy stub | ST-10-4 | Tail bounded and sanitised | NFT-SEC-38 |
| AC-8 | DS-16, DS-03, DS-08, DS-35 | AT-10-1, AT-10-2 | As `tests.md` | NFT-SEC-28, FT-N-06, FT-N-07 |

## Constraints

- Harness H3 and the stub variant from E-19/c15; reference hardware from the E-31/c01 inventory.

## Risks & Mitigation

**Risk 1: Reference hardware unavailable**
- *Risk*: PT-10-2 cannot run.
- *Mitigation*: recorded `NOT-RUN(HW-REF)` in `_docs/_process_leftovers/`; no workstation substitute is reported.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None beyond those of the tasks under test | — | — | — |
