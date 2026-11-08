# Lease heartbeat, two-tier watchdog, unhealthy slot and lease-lost abort

**Task**: AZ-3873_worker_heartbeat_watchdog
**Name**: Worker heartbeat and watchdog
**Description**: Keep each running job's lease alive at `lease_ms / 3`, abort quietly when the lease is lost, cancel a silent or over-time decoder and mark its slot unhealthy, and recover the slot only through reconnect and the self-check gate
**Complexity**: 3 points
**Dependencies**: AZ-3872_worker_slot_loop
**Component**: 09_worker_supervisor
**Tracker**: AZ-3873
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — worker listener routes and statuses, produced by E-18/c02.

## Problem

A job whose lease expires is handed to another worker; a supervisor that keeps going would post a second result (AC-71). A decoder can hang without any progress line (tier 1, the runner's stall kill, only covers ffmpeg) or keep sending `Alive` while doing nothing; Docker does not restart on a failed health check, so the supervisor must stop using such a slot itself (AC-45, R34).

## Outcome

- Heartbeats run while a job runs and stop with it.
- A lost lease cancels the decoder within 1 s and nothing more is posted for that job.
- A silent or over-time decoder is cancelled, its slot stops claiming and reports not ready, and the job is failed `Retry`.

## Scope

### Included
- Heartbeat per running job every `lease_ms / 3` from `ClaimedJob.lease_ms`; stopped on every exit path.
- `LeaseLost` from a heartbeat: `cancel` to the decoder, no output, no result, no failure, scratch removed, slot keeps polling.
- Tier 2: no frame of any kind for `decoder_silence_timeout` -> `cancel`, wait `decoder_cancel_grace`, slot unhealthy (no claims, `/readyz` 503), `fail(Retry)`.
- Wall limit per kind (`photo_job_timeout`, `video_job_timeout`) enforced here as well, with the same outcome.
- A decoder protocol violation (from E-20/c05): slot unhealthy, `fail(Retry)` for the current job, reconnect and gate before the slot claims again.
- Slot back to healthy only after reconnect, `hello` and an all-ok `self_check`.

### Excluded
- Tier 1 (`decoder_stall_kill` inside the runner, E-19/c06); the retry rules (E-20/c06).

## Acceptance Criteria

**AC-1: Heartbeat cadence (IT-09-10, IT-09-2)**
Given `job_lease = 6s` and a fake decoder job lasting 8 s
When the job runs
Then lease extensions are sent every 2 s +/- 500 ms and none after the job ends

**AC-2: Lease lost (IT-09-10 case A, AC-71)**
Given a running job whose lease the test expires
When the next heartbeat gets 409
Then `cancel` reaches the decoder within 1 s, no output, result or failure request is sent for that job, its scratch is removed, and the slot keeps polling with the same worker id

**AC-3: Silent decoder (IT-09-11, AC-45)**
Given `decoder_silence_timeout = 1s`, `decoder_cancel_grace = 500ms` and a decoder that stops sending frames
When the slot waits
Then `cancel` is sent at about 1 s, after 500 ms the slot is unhealthy (no further claims for it, `/readyz` 503), and the job is failed `Retry` (job back in the queue)

**AC-4: Wall limit (IT-09-11)**
Given `photo_job_timeout = 2s` and a decoder that keeps sending `Alive` but never answers a render
When the job runs
Then `cancel` is sent at or before 2.5 s with the same outcome as AC-3

**AC-5: Violation and recovery (IT-09-13, IT-09-15)**
Given a decoder that sends a hostile frame, and later accepts a new connection
When the violation is detected and the decoder restarts
Then the current job is failed `Retry`, the slot is unhealthy until `hello` and an all-ok `self_check` pass on the new connection, and only then claims again

## Non-Functional Requirements

**Reliability**
- A slot is never blocked beyond `decoder_silence_timeout` plus `decoder_cancel_grace`, or beyond the wall limit (09 NFR "Tier-2 silence").

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Heartbeat schedule from `lease_ms` | Interval `lease_ms / 3`; stops at end |
| AC-3, AC-4 | Watchdog with short durations from config | Cancel, grace, unhealthy, `fail(Retry)` |
| AC-5 | Slot health state changes | Healthy only after the gate |

## Blackbox Tests

Run in harness H2 (real API, test clock, FD) in E-20/c11.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | FD 8 s job, clock advance | IT-09-10 case A | Cadence; silent stop | AC-45, AC-71 |
| AC-3, AC-4 | Silent FD; `Alive`-only FD | IT-09-11 | Cancel, unhealthy, `Retry` | AC-45, NFT-RES-19 |
| AC-5 | Hostile FD; reconnect | IT-09-13, IT-09-15 | Unhealthy until gate passes | AC-25 |

## Constraints

- Durations come from config; tests use millisecond values (testing rule).

## Risks & Mitigation

**Risk 1: Late post after a lost lease**
- *Risk*: a post already in flight completes after the 409.
- *Mitigation*: the server's `accept_output` is idempotent and lease-checked (E-18/c07); AZ-3661 AC-71 counts rows end to end.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
