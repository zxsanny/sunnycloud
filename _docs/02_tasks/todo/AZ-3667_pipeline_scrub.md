# Scrub pipeline (F15)

**Task**: AZ-3667_pipeline_scrub
**Name**: Scrub pipeline
**Description**: The scheduled scrub and `scrub run` re-verify stored blobs, report corruption through findings, metrics, health and audit, and stay safe beside live uploads and GC, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3784_sc_audit, AZ-3786_sc_observability, AZ-3832_ops_endpoints_readiness, AZ-3789_composition_root, AZ-3792_janitor_scheduler, AZ-3798_scrub_commands, AZ-3730_gc_sweeper, AZ-3745_scrubber, AZ-3746_blob_maintenance_ports, AZ-3821_admin_health_aggregation, AZ-3735_transfer_put_block, AZ-3755_transfer_download, AZ-3655_pipeline_large_upload, AZ-3663_pipeline_gc_retention_restore, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3667
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p14

## Problem

Scrub runs in two places, the API process (`ScrubJanitor` under the scheduler, paused by the activity gauge) and the CLI (`scrub run`), and reports through 04's findings, the audit log, metrics and 07's health aggregation of the `HealthContributor`s that 12 registers. Its safety rule (AC-80) depends on uploads and GC running at the same time in the real process. No planned task proves this through the running server.

## Outcome

- Scheduled and CLI scrub runs verify every committed blob and report corruption as findings, a metric, a degraded health state and an audit event.
- Scrub beside live uploads and GC reports only real corruption.
- Every component operation of flow F15 has a production caller; at least one integration test runs F15 through the real binaries.

## Scope

### Included
- Flow order and seams (`system-flows.md` F15 steps 1-5):
  1. Scheduler (12) → `ScrubJanitor::run_cycle`; operator → `scrub run [--full]` → `Scrubber::{start_run, run_step, finish_run}`; `scrub status` → `Scrubber::status`.
  2. `run_step` reads ciphertext under the rate cap; in the API process it pauses while the activity gauge (active block writes and downloads) is high.
  3. AEAD and length checks; on mismatch the row is re-read and reported only if still committed → `unavailable_at` set → `scrub_mismatch` audit.
  4. `BlobHealth` as a `HealthContributor` → 07 health aggregation → `/healthz` degraded; scrub metrics.
  5. `finish_run` → one `scrub_summary` audit event; `scrub run` exits 2 on a mismatch or missing blob; downloads of a flagged blob fail cleanly (F6).
- The end-to-end integration test of the steps above.

### Excluded
- Automatic repair (not in v1.0), the transfer-throughput measurement on REF-SRV (E-31/c43).

## Acceptance Criteria

AC-1..AC-5 are local to this task. AC-80 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Clean runs**
Given a volume of committed originals and derived blobs
When the scheduled scrub cycle runs and `scrub run --full` runs
Then both finish with no finding, `scrub run` exits 0, and `scrub status` shows the oldest scrub time, the unscrubbed count and zero open findings

**AC-2: Corruption is reported everywhere it should be**
Given one blob with a flipped byte and one blob file removed
When `scrub run --full` runs
Then it exits 2, two findings (corrupt, missing) are recorded with ids, the scrub metric rises, `/healthz` reports degraded, a `scrub_mismatch` audit event exists, and a download of the corrupt file fails cleanly without wrong bytes

**AC-3: One summary per run**
Given any finished run (scheduled or CLI)
When the audit log is read
Then it holds exactly one `scrub_summary` event for the run with integer counts and duration

**AC-4: In-process scrub yields to transfers**
Given a scheduled cycle and active uploads above the pause threshold
When the cycle runs
Then the scrub pauses while the gauge is above the threshold and resumes after

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `Scrubber::{start_run, run_step, finish_run, status}` (commands), a scheduler registration of `ScrubJanitor`, `BlobHealth` registered as a `HealthContributor`, and the activity gauge fed by the transfer paths in the API process

### Interaction criteria (Step 1.8)

**AC-80: Scrub never flags in-flight or collected blobs** (R13)
Given uploads in progress (blobs in `uploading`, blocks being written) and the GC removing due blobs
When a full scrub runs at the same time, both in the API process and from the CLI
Then no blob that is being uploaded or removed by the GC is reported corrupt or missing, and only the seeded corruptions of AC-2 are reported

## Non-Functional Requirements

**Performance**
- Transfers keep at least 90% of baseline throughput during a scrub (AC-24); measured on REF-SRV in E-31.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources | Each named call and registration has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Committed originals and derived | Scheduled cycle, `scrub run --full`, `status` | No findings, exit 0 | AC-24 |
| AC-2 | One flipped byte, one missing file | `scrub run --full`, health, audit, download | Exit 2, findings, metric, degraded, audit, clean download failure | AC-7 |
| AC-3 | Finished runs | Read audit | One summary per run | |
| AC-4 | Active uploads above threshold | Scheduled cycle | Pauses and resumes | AC-24 |
| AC-80 | Uploads in progress, GC running | Concurrent full scrub (API and CLI) | Only seeded corruptions reported | NFT-RES-39 |

## Constraints

- Owner: component 12 (`src/server/`): scheduler and health registrations for this flow, the commands' wiring, and the flow's end-to-end tests among 12's integration tests. No blob I/O in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume; corruption is seeded on the volume, not inside a component.
- The race handling itself stays in E-10/c12; AC-80 checks it with real uploads and GC.
- Downstream: E-31/c28 (FT-P-43) and E-31/c41 (NFT-RES-39) should depend on this task.

## Risks & Mitigation

**Risk 1: Scrub starves uploads**
- *Risk*: the gauge is not fed in the API process, so the scheduled scrub never pauses.
- *Mitigation*: AC-4 runs the cycle under upload load.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Automatic repair is out of v1.0 (open question 9) | Owner | accepted-risk | Low |
