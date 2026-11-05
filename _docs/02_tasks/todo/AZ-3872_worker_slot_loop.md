# Per-slot work loop: lanes, stills/poster/transcode flow, retry inside the claim, failure codes

**Task**: AZ-3872_worker_slot_loop
**Name**: Worker slot loop
**Description**: Drive one job at a time per decoder slot from claim to terminal state: lane choice, fetch, probe, the step chosen by the policy step, renders and transcodes, posts, one CPU rerun for stalled or failed encodes, and the mapping of every outcome to complete, fail Retry or fail Permanent
**Complexity**: 5 points
**Dependencies**: AZ-3869_worker_api_client, AZ-3870_worker_scratch, AZ-3871_worker_decoder_client, AZ-3876_worker_policy_step
**Component**: 09_worker_supervisor
**Tracker**: AZ-3872
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — worker listener routes and statuses, produced by E-18/c02.
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — calls and `DecodeError` variants used by the loop.
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `JobKind`, `Lane`, `DerivedKind`, `SkipOutcome`, `Disposition`.

## Problem

The supervisor is the only trusted driver of a job (F10). It must keep the reserved interactive slot free for photos (AC-10), never burn three attempts on hostile media (permanent codes), give a stalled or failed encode exactly one CPU rerun (AC-45, AC-28), react to 404/409/403 as 09 §2 states, and survive a kill at any step without leaving a duplicate or residue (AC-71, component part).

## Outcome

- Each slot claims from its lanes, runs one job at a time and always ends with scratch removed and heartbeat stopped.
- Every outcome ends as the documented call: outputs posted, `complete`, `fail(Retry)` or `fail(Permanent)`, or nothing after a lost lease.
- A retried job whose outputs were partly stored completes, because already-present outputs count as success.

## Scope

### Included
- Slots: interactive slot claims lane `image` only; general slot claims `video` and `image` with the first lane rotated each claim; empty claim (204) waits `worker_claim_poll_interval`.
- Flow: `claim` -> `Scratch::create` -> input-fit check -> `fetch_input` -> `probe` -> the step from the policy step (E-20/c10a):
  - `stills`: `render_still{1440}`, post `preview`, clear `out/`, `render_still{320}`, post `thumb` (one fetch for both);
  - `poster`: `render_still{320, poster_at_ms}`, post `thumb`;
  - `transcode`: `SkipCompliant` -> `complete(skipped_compliant)` with no output; `Remux` or `Transcode(Plan)` -> `transcode`, output check, post `transcode`.
- Limits per kind sent with every request (photo 2 CPU / 2 GiB / 120 s; video 4 CPU / 4 GiB / 40 min; `stall_timeout = decoder_stall_kill`).
- Retry inside the claim: `Stalled` or `EncoderFailed` -> one rerun with a cleared `out/` and `encoder: Cpu`; a failed `Remux` reruns once as a CPU `Transcode`.
- Failure mapping: permanent (`Unsupported`, `Corrupt`, `LimitExceeded`, `OutputInvalid`, `input_too_large`, `output_rejected`, second `Stalled`) -> `fail(Permanent)`; transient (decoder unreachable or killed, fetch or post `Transient`, `HashMismatch`) -> `fail(Retry)`.
- API reactions: `Gone` -> `complete(obsolete)`, no decoder call, no retry; `LeaseLost` -> stop, post nothing more, no result, no failure; `Forbidden` -> worker not ready, stop claiming; post answered "already present" -> success.
- Always: scratch removed, heartbeat stopped, a metrics event emitted (families in E-20/c08).

### Excluded
- Heartbeat timing, the silence and wall watchdogs and slot health (E-20/c07); the decision itself (E-20/c10a); status, shutdown and composition (E-20/c08).

## Acceptance Criteria

**AC-1: Lanes and one job per slot (AC-10, IT-09-9)**
Given 20 queued `poster`/`transcode` jobs and 20 `stills` jobs, and two slots whose decoders answer after 300 ms
When the slots run
Then every interactive claim has `lane=image`, the general slot's lanes alternate `video, image, ...` while both have work (at least 4 of each in its first 10 claims), and no slot has two jobs at once

**AC-2: One fetch, two renders (AC-10, IT-09-2)**
Given a `stills` job
When it runs
Then there is exactly one input fetch, renders `max_edge 1440` then `320`, posts `preview` then `thumb`, `out/` is empty between the renders, and the job tree is removed afterwards

**AC-3: Poster and transcode steps executed (IT-09-3)**
Given a `poster` job and three `transcode` jobs whose steps are `SkipCompliant`, `Remux` and `Transcode`
When they run
Then the poster render is posted as `thumb`; `SkipCompliant` sends `complete(skipped_compliant)` with no transcode call and no output request; `Remux` and `Transcode` send `transcode` with that plan and post `transcode`

**AC-4: Retry inside the claim (AC-45, IT-09-6)**
Given decoder answers per job: `Stalled` then ok; `Stalled` twice; `EncoderFailed{1}` on an `Nvenc` plan then ok; a failing `Remux` then ok
When each job runs
Then the first reruns once with a cleared `out/` and `encoder: Cpu` and succeeds; the second fails `Permanent`; the third reruns with `Cpu`; the fourth reruns as a CPU `Transcode`, not `Remux`

**AC-5: Permanent and transient codes (IT-09-6, IT-09-7)**
Given answers `Unsupported`, `Corrupt`, `LimitExceeded`, a socket closed mid-job, a fetch 5xx, and a `HashMismatch`
When each job runs
Then the first three fail `Permanent` at the first attempt, the last three fail `Retry`, and no `HashMismatch` input reaches the decoder

**AC-6: API reactions (IT-09-8, IT-09-10)**
Given a fetch answering 404, a call answering 409, and a call answering 403
When each happens
Then 404 sends `complete(obsolete)` with no decoder call and no retry; 409 sends nothing more for that job (no output, result or failure); 403 marks the worker not ready and no further claim is sent

**AC-7: AC-71 component part: retry after a kill completes once**
Given a `stills` job whose `preview` was stored by an earlier attempt
When the retried attempt posts `preview` and gets "already present", then posts `thumb`
Then the attempt treats `preview` as done, posts `thumb`, and the job tree is removed

**AC-8: Limits per kind (AC-25)**
Given a photo job and a video job
When their decoder requests are recorded
Then the photo carries 2 CPU, 2 GiB, 120 s and the video 4 CPU, 4 GiB, 40 min, both with `stall_timeout` equal to `decoder_stall_kill`

## Non-Functional Requirements

**Performance**
- Supervisor time per photo job above decoder time: target 40 ms, failure 150 ms; 6 jobs/s with a 50 ms/render fake decoder (failure 3) (PT-09-1, measured in E-20/c12).

**Reliability**
- A slot is never blocked beyond the stall or wall limit (AC-45; watchdogs in E-20/c07).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Slot lane masks and rotation | Interactive image-only; general alternates |
| AC-4, AC-5 | Outcome-to-action table | Rerun, `Permanent` or `Retry` per code |
| AC-6 | 404, 409 and 403 reactions | As specified |
| AC-7 | Already-present answer | Treated as success |
| AC-8 | Request limits per job kind | Photo and video limits as configured; `stall_timeout = decoder_stall_kill` |

## Blackbox Tests

Run in harness H2 (real API, FD or RD) in E-20/c11.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 40 jobs, two FD sockets | IT-09-9 | Lanes as specified | AC-10, FT-P-21 |
| AC-2 | DS-03 JPEG, RD interactive | IT-09-2 | One fetch; preview then thumb | AC-10, FT-P-20 |
| AC-3 | FD scripted probes | IT-09-3 | Steps executed | AC-11 |
| AC-4, AC-5 | FD scripted outcomes | IT-09-6 | Rerun and dispositions | AC-45, FT-N-06, NFT-RES-19 |
| AC-6 | Deleted revision; lease expired; token replaced | IT-09-8, IT-09-10 | `obsolete`; silent stop; not ready | AC-79 |
| AC-7 | Kill during post, restart | IT-09-14 post case | Retry completes | AC-71 |

## Constraints

- The loop's type is the implementer's choice (G-09-1, E-20/c01); tests observe only the API, socket, scratch and ops port.
- In memory only; nothing to rehydrate after a restart (09 §4).

## Risks & Mitigation

**Risk 1: A poster waits behind a transcode**
- *Risk*: the one `video`-capable slot is busy for up to 40 min.
- *Mitigation*: accepted for v1.0 (09 risk 6); a second supervisor pair adds a slot.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The disposition after a second `EncoderFailed` (CPU rerun also failing) is not listed in 09 §3; only a second `Stalled` is named permanent | E-20 owner | open | Low |
