# Supervisor main, status report, metrics and graceful shutdown

**Task**: AZ-3874_worker_status_shutdown_main
**Name**: Worker main, status, metrics and shutdown
**Description**: Compose the `sunnycloud-worker` process (config, logging, sweep, ops listener, both slots with the real `WorkerApi`, `Scratch` and `DecoderClient`, status reporter), report decoder health and image age, register the worker metrics, and drain on SIGTERM
**Complexity**: 3 points
**Dependencies**: AZ-3868_worker_crate_skeleton, AZ-3869_worker_api_client, AZ-3870_worker_scratch, AZ-3871_worker_decoder_client, AZ-3872_worker_slot_loop, AZ-3873_worker_heartbeat_watchdog, AZ-3876_worker_policy_step, AZ-3765_sc_clock, AZ-3786_sc_observability
**Component**: 09_worker_supervisor
**Tracker**: AZ-3874
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — worker listener routes and statuses, produced by E-18/c02.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `SystemClock`, produced by E-03/c01.
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — metrics registry, produced by E-09/c01.

## Problem

Every part of the supervisor exists as a separate piece; nothing yet starts them in the right order with the real clients. The Step 1.7 pipeline (AZ-3661, AC-5) requires the supervisor's `main` to start its slot loops with the real `WorkerApi` and `DecoderClient`, and no E-20 row owned that composition; it is added here. The operator's `doctor` needs the worker's status (decoder health, image age), Prometheus needs the worker metrics, and a deploy must not leave half-done jobs (SIGTERM).

## Outcome

- One start sequence: no claim before scratch is swept and the slot's gate has passed.
- Status reported every interval; metrics registered once with enumeration labels.
- SIGTERM stops claiming, lets short jobs finish within the grace, cancels and releases the rest, and exits 0.

## Scope

### Included
- Composition (constructor injection, no statics): load the config section; init logging; `sweep_all` both roots; start the ops listener; for each slot build `DecoderClient` on its socket, `Scratch` on its root and the slot loop with its lanes (interactive `image`; general `video` + `image`); one shared `WorkerApi`; the clock (`SystemClock`; the test build's offset clock is wired by E-20/c09); start the status reporter.
- Status every `worker_status_interval`: `decoder_ok` (every slot passed `self_check` and is healthy) and `image_age_days` from the decoders' `build_timestamp` against the clock; a `Transient` failure is logged and retried at the next interval; after a 403 the reports stop.
- Metrics: `jobs_total{kind,result}` (results include `obsolete`, `remuxed`), `job_duration_seconds`, `decoder_kills_total{reason}`, `thumbnail_ready_seconds`, `worker_selfcheck_ok`, `worker_decoder_build_timestamp_seconds`, `worker_decoder_restarts_total`.
- SIGTERM: stop claiming, let running jobs finish for `worker_shutdown_grace`, then `cancel` + `release` for the rest, then exit 0.
- Log fields `job_id`, `kind`, `attempt`, `slot`, `err_code`; never paths.

### Excluded
- The offset clock poll (E-20/c09); NVENC preference (E-20/c10b); the pieces themselves (E-20/c02 to c07, c10a).

## Acceptance Criteria

**AC-1: Start order (IT-09-1)**
Given leftover scratch, a decoder whose gate passes on the general slot and fails on the interactive slot
When the worker starts
Then both roots are empty before the first claim, the general slot claims, the interactive slot claims nothing, and `/readyz` answers 503

**AC-2: Real clients in production code (AZ-3661 AC-5)**
Given the worker source
When the wiring check searches non-test code
Then `main` builds the slot loops with the real `WorkerApi` and `DecoderClient`, and no test double is reachable from it

**AC-3: Status report (IT-09-17)**
Given `worker_status_interval = 1s` and a decoder `build_timestamp` 20 days before the clock
When the worker runs for 3 s
Then a status request with `decoder_ok = true` and `image_age_days = 20` is sent every second; with a failing gate the body has `decoder_ok = false`; a status `Transient` is logged and the next interval sends again

**AC-4: Graceful shutdown (IT-09-17)**
Given `worker_shutdown_grace = 3s`, a running 8 s job and a running 1 s job
When SIGTERM arrives
Then no new claim is sent, the 1 s job completes and posts, the 8 s job gets `cancel` at 3 s and a lease release, and the process exits 0 within 5 s

**AC-5: Metrics (09 §8)**
Given a run with one `stills` success, one `obsolete`, one remux and one decoder kill for memory
When `/metrics` is read
Then `jobs_total` shows each result under its `kind`, `decoder_kills_total{reason="memory"}` is 1, `worker_selfcheck_ok` and `worker_decoder_build_timestamp_seconds` are set, and every label value is from a fixed enumeration

## Non-Functional Requirements

**Performance**
- Supervisor RSS target 64 MiB, failure 128 MiB (PT-09-1, measured in E-20/c12).

**Reliability**
- A restart needs no rehydration: lease expiry returns in-flight jobs (09 §4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | `image_age_days` from timestamps with a manual clock | Whole days, correct rounding |
| AC-4 | Shutdown sequence with short grace | Stop claims; finish; cancel and release; exit 0 |
| AC-2 | Static wiring check over non-test sources | Real clients in `main` |
| AC-5 | Metric families and label values after a scripted run | Counts per result; labels from fixed enumerations |

## Blackbox Tests

Run in harness H2 (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Leftovers; gate passing and failing | IT-09-1 | Sweep first; claims only on ready slots | AC-25 |
| AC-3, AC-4 | FD jobs of 8 s and 1 s; build timestamp 20 days old | IT-09-17 | Status body; drain on SIGTERM | AC-45, AC-71 |

## Constraints

- No `contracts` port, no database: the worker is a leaf (ADR-015).

## Risks & Mitigation

**Risk 1: Claims before the gate**
- *Risk*: a slot loop starts before its decoder is proven isolated.
- *Mitigation*: AC-1 orders sweep, gate, then claim.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No E-20 child row owned the composition of `main` that AZ-3661 assumes (its flagged concern points to E-20/c06); added to this row, planned 2 points raised to 3 | Parent (update AZ-3661) | open | Low |
| `thumbnail_ready_seconds` is named without its start point (the supervisor never sees the upload time) | E-20 owner | open | Low |
