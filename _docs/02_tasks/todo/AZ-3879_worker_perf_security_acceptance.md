# Worker performance, security and acceptance tests PT-09-1..3, ST-09-1..3, AT-09-1..2

**Task**: AZ-3879_worker_perf_security_acceptance
**Name**: Worker perf, security and acceptance tests
**Description**: Measure supervisor overhead, two-supervisor scaling and streaming memory; prove the worker secret leaks nowhere, the container has no path to the database or blobs, and decoder outputs cannot exfiltrate files; run the two component acceptance scenarios
**Complexity**: 3 points
**Dependencies**: AZ-3878_worker_integration_suite; notes: E-31/c19 (test epic E-31, decomposed at greenfield Step 9); E-31/c21 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 09_worker_supervisor
**Tracker**: AZ-3879
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/components/09_worker_supervisor/tests.md` — PT-09-1..3, ST-09-1..3, AT-09-1..2.

## Problem

The supervisor must cost little beside the decoder (AC-10 bulk budget), scale with more supervisors (AC-11), stream large inputs without buffering, and keep its single secret and its lack of data access provable (AC-25, AC-79). The acceptance scenarios show the policy outcome and the kill-and-recover property as the component owner states them.

## Outcome

- PT-09-1..3 measured against their targets and failure thresholds.
- ST-09-1..3 and AT-09-1..2 pass on harness H2.

## Scope

### Included
- PT-09-1 overhead on 2,000 `stills` jobs with FD at 50 ms/render; PT-09-2 throughput with 1 then 2 supervisors on 400 jobs; PT-09-3 streaming RSS with DS-25 (1 GiB).
- ST-09-1 secret handling; ST-09-2 no path to the database or blobs; ST-09-3 output exfiltration with the 10,000-swap race.
- AT-09-1 compliant clip leaves no output, index-at-end clip remuxed; AT-09-2 kill-and-recover with two supervisor pairs.

### Excluded
- System budgets on reference hardware (NFT-PERF-14, NFT-PERF-17, E-31).

## Acceptance Criteria

**AC-1: Overhead (PT-09-1, AC-10)**
Given 2,000 `stills` jobs of DS-03 size and FD answering each render in 50 ms
When one supervisor processes them
Then supervisor time per job above FD time is at most 150 ms (target 40 ms), throughput at least 3 jobs/s (target 6), RSS at most 128 MiB (target 64 MiB), and supervisor CPU under 50% of one core

**AC-2: Scaling (PT-09-2, AC-11)**
Given 400 jobs and FD taking 200 ms each
When they run with one and then two supervisors on the same queue
Then the throughput ratio is at least 1.7 (target 1.9) and duplicate claims are 0

**AC-3: Streaming memory (PT-09-3, AC-72)**
Given DS-25 (1 GiB) as a video input with the default input limit
When it is fetched
Then peak RSS is at most 128 MiB (target 64 MiB) and the hash equals the manifest

**AC-4: Secret appears only in the header (ST-09-1, AC-79)**
Given `worker_api_token` set to a 48-character marker
When IT-09-2, IT-09-10 case B and IT-09-13 run and logs, `/metrics`, `/readyz` bodies, status bodies and API-side logs are searched
Then the marker has 0 matches outside the Authorization header

**AC-5: No path to data (ST-09-2, AC-25)**
Given the worker container
When env, mounts, secrets, capabilities and networks are inspected and a startup message for role `sunnycloud` with an empty password is sent to `db:5432`
Then there is no database variable, only `worker_api_token` under the secrets folder, no blob mount, no Docker socket, and authentication fails for any role

**AC-6: No exfiltration through outputs (ST-09-3)**
Given the IT-09-12 cases and an FD that swaps `out/result.webp` between a regular file and a symlink 10,000 times during reads
When 100 runs complete
Then 0 posted bodies contain the secret marker; each posted body is the regular file's bytes or nothing

**AC-7: Acceptance (AT-09-1, AT-09-2)**
Given the core stack with real worker and decoders, DS-07 and the DS-35 moov-at-end clip; and two supervisor pairs with 20 transcode jobs from the DS-08 subset
When the clips are uploaded; and one supervisor is killed at random during decode and restarted
Then DS-07 closes `skipped_compliant` with no transcode served and the moov-at-end clip has a remuxed output; and every job ends `succeeded` or `dead` with a hostile-class code, exactly one derived blob exists per key, and both scratch volumes are empty

## Non-Functional Requirements

**Performance**
- The thresholds above (proposed targets from `tests.md`; system budgets are NFT-PERF-14 and NFT-PERF-17 on REF-CPU4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (test task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 2,000 jobs, FD 50 ms | PT-09-1 | Within thresholds | NFT-PERF-14 |
| AC-2 | 400 jobs, FD 200 ms, 1 vs 2 supervisors | PT-09-2 | Ratio >= 1.7; 0 duplicates | NFT-PERF-17 |
| AC-3 | DS-25 | PT-09-3 | RSS bound; hash equal | FT-N-43 |
| AC-4 | Marker token | ST-09-1 | 0 matches | NFT-SEC-38, NFT-SEC-50 |
| AC-5 | Worker container | ST-09-2 | No data path | NFT-SEC-29 step 4-5, NFT-SEC-46 |
| AC-6 | Swap-race FD | ST-09-3 | 0 secret posts | AC-25 |
| AC-7 | DS-07, DS-35, DS-08 subset | AT-09-1, AT-09-2 | As `tests.md` | NFT-RES-36, AC-71 |

## Constraints

- Harness H2 from E-20/c11; FD only where `tests.md` scripts it, RD for every media outcome.

## Risks & Mitigation

**Risk 1: AT-09-2 overlaps the pipeline's AC-71**
- *Risk*: the same property is asserted twice in different shapes.
- *Mitigation*: AT-09-2 is the component acceptance with two supervisor pairs; AZ-3661 owns the end-to-end AC-71 with the derived janitor; both stay, neither replaces the other.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None beyond those of the tasks under test | — | — | — |
