# Worker integration suite IT-09-1..18 on harness H2, including kill at each step

**Task**: AZ-3878_worker_integration_suite
**Name**: Worker integration suite (H2)
**Description**: Run the supervisor's test cases against the real API worker listener and PostgreSQL in the test stack, with a scripted fake decoder on the real socket for faults and the real decoder for every media outcome, including SIGKILL of the supervisor at fetch, decode and post
**Complexity**: 5 points
**Dependencies**: AZ-3868_worker_crate_skeleton, AZ-3869_worker_api_client, AZ-3870_worker_scratch, AZ-3871_worker_decoder_client, AZ-3872_worker_slot_loop, AZ-3873_worker_heartbeat_watchdog, AZ-3874_worker_status_shutdown_main, AZ-3875_worker_offset_clock_poll, AZ-3876_worker_policy_step, AZ-3852_decoder_protocol_module, AZ-3853_decoder_runner_socket_loop, AZ-3855_decoder_containment_kill_all, AZ-3856_decoder_safe_open_watchdog, AZ-3857_decoder_image_executor, AZ-3858_decoder_probe, AZ-3860_decoder_video_executor, AZ-3861_decoder_hello_self_check, AZ-3863_decoder_media_build_stages, AZ-3840_job_queue, AZ-3842_derived_store_accept_input, AZ-3843_worker_auth_jobs_facade, AZ-3831_internal_router_test_clock, AZ-3789_composition_root, AZ-3791_serve_start_stop; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); E-31/c07 (test epic E-31, decomposed at greenfield Step 9); E-31/c09 (test epic E-31, decomposed at greenfield Step 9); E-31/c10 (test epic E-31, decomposed at greenfield Step 9); E-31/c20 (test epic E-31, decomposed at greenfield Step 9); E-31/c21 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 09_worker_supervisor
**Tracker**: AZ-3878
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — the fake decoder implements the server side with the shared module.
- `_docs/02_document/components/09_worker_supervisor/tests.md` — harness H2, IT-09-1..18 expected results.

## Problem

The supervisor's behaviour only matters against its real peers: the API's worker listener decides leases, attempts and `AlreadyPresent`, and the decoder socket carries hostile frames. The component tests of E-20 use local peers; this suite proves the same behaviour against the real listener and database, and owns the kill-at-each-step case that keeps the supervisor's part of AC-71 (interaction risk R4: "E-20/c11 kill-at-each-step").

## Outcome

- Harness H2 runs: real API (08 behind 11) with PostgreSQL 18, the supervisor `-test` image, the fake decoder FD on the real socket (faults and protocol cases only) and the real decoders RD (every media outcome).
- IT-09-1 to IT-09-15, IT-09-17 and IT-09-18 pass (IT-09-16 is v1.1, E-20/c10b).
- Kill at fetch, decode and post leaves an empty scratch, a reclaimed job and a cancelled decoder.

## Scope

### Included
- FD: a scripted decoder implementing the protocol's server side from the shared module, recording every request, able to send hostile frames and files.
- H2 config from `tests.md` (`job_lease=6s`, `worker_claim_poll_interval=100ms`, `worker_status_interval=1s`, `decoder_silence_timeout=1s`, `decoder_cancel_grace=500ms`, `photo_job_timeout=5s`, `worker_shutdown_grace=3s`); per test: golden snapshot or fresh account, empty scratch, clock offset at the test's start value.
- All cases of IT-09-1 to IT-09-15, IT-09-17, IT-09-18 with the expected results of `tests.md`; the G-09-2, G-09-3 defaults (`input_too_large`, `output_rejected`) and the G-09-4 effect from E-20/c01 asserted.
- IT-09-14 component part of AC-71 (below); the end-to-end count of derived rows and blobs stays with AZ-3661.

### Excluded
- PT-09-x, ST-09-x, AT-09-x (E-20/c12); IT-09-16 (E-20/c10b); the end-to-end media pipeline (AZ-3661).

## Acceptance Criteria

**AC-1: Start-up and reconnect (IT-09-1, IT-09-15, AC-25)**
Given pre-filled scratch roots, FD with all self-check items ok and then with one failed, and RD whose runner is killed while idle
When the supervisor starts and the runner restarts
Then IT-09-1 and IT-09-15 meet their expected results (sweep before first claim; no claim on a failed gate; `hello`, `self_check`, then claim after a reconnect)

**AC-2: Job flows with the real decoder (AC-10, AC-11, IT-09-2, IT-09-4)**
Given RD and DS-03, DS-07, DS-35
When the `stills`, compliant, portrait and moov-at-end jobs run
Then one fetch and preview-then-thumb hold for the photo, compliant clips close `skipped_compliant` with 0 bytes posted, and the moov-at-end clip yields a remuxed output whose packet hash equals the input's

**AC-3: Decisions, compliance and obsolete (IT-09-3, IT-09-5, IT-09-8)**
Given FD scripted probes and outputs and a revision deleted before fetch
When the jobs run
Then poster offsets and decisions, the `OutputInvalid` single attempt and the `obsolete` close meet their expected results

**AC-4: Faults (IT-09-6, IT-09-7, IT-09-10, IT-09-11, IT-09-12, IT-09-13)**
Given the FD fault scripts, the edited job hash, the limits, the expired lease, the replaced token, the silent and over-time decoders and the hostile output files and frames of `tests.md`
When each case runs
Then each meets its expected result, including no secret byte in any posted body and the supervisor process staying up

**AC-5: Slots (IT-09-9)**
Given 40 queued jobs and two FD sockets
When the supervisor runs
Then lane sequences and per-slot concurrency meet IT-09-9

**AC-6: AC-71 component part: kill at each step (IT-09-14, R4)**
Given RD with a DS-05 base transcode job and a DS-03 photo job, and `ctl` killing the supervisor container during the input fetch, during `transcode`, and during the output post (one run each)
When the supervisor is restarted after each kill
Then both scratch roots are empty before the first claim, the job is reclaimed after lease expiry with attempt 2 and ends `succeeded`, outputs answered "already present" are treated as success, no ffmpeg process remains in the decoder container, and the killed run's decoder job was cancelled by socket loss within 2 s

**AC-7: Shutdown, status and test clock (IT-09-17, IT-09-18)**
Given the jobs and clock settings of IT-09-17 and IT-09-18
When SIGTERM is sent and the test clock is advanced
Then both cases meet their expected results, with the G-09-4 effect from E-20/c01 observed in the test image and no offset request from the release image

## Non-Functional Requirements

**Reliability**
- Each case keeps its "Max execution time" from `tests.md`; IT-09-14 at most 180 s.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | FD script parser and request recorder | Scripts replay exactly; every request recorded |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Leftovers; FD gate variants; RD restart | IT-09-1, IT-09-15 | As `tests.md` | NFT-SEC-29, AC-25 |
| AC-2 | RD, DS-03/07/35 | IT-09-2, IT-09-4 | As `tests.md` | FT-P-20, FT-P-25, AC-10, AC-11 |
| AC-3 | FD probes, deleted revision | IT-09-3, IT-09-5, IT-09-8 | As `tests.md` | FT-P-26, AC-44 |
| AC-4 | FD faults, clock, token swap | IT-09-6, 7, 10, 11, 12, 13 | As `tests.md` | FT-N-06, FT-N-43, NFT-RES-19, NFT-SEC-50 |
| AC-5 | Two FD sockets | IT-09-9 | As `tests.md` | FT-P-21 |
| AC-6 | RD, DS-05 base, DS-03, ctl kills | IT-09-14 | Empty scratch; reclaim; decoder cancelled | AC-71, NFT-RES-23, NFT-RES-36 |
| AC-7 | FD 8 s and 1 s jobs; test and release images | IT-09-17, IT-09-18 | As `tests.md` | FT-N-35 |

## Runtime Completeness

- Named capability: the real API worker listener with PostgreSQL 18, and the real decoder images for media outcomes.
- Allowed external stubs: FD, only for cases that script decoder faults or protocol behaviour (`tests.md` H2 rule).
- Unacceptable substitutes: a fake HTTP server in place of the API, an in-memory queue, or FD in any case that claims a media outcome.

## Constraints

- Real API, queue and PostgreSQL in every case (`tests.md` data isolation); this resolves the E-20 epic's ordering note by depending on the worker routes (E-21/c15, E-18) instead of faking them.
- Container-level tiers run on the host named in `tests/environment.md`.

## Risks & Mitigation

**Risk 1: Kill timing is flaky**
- *Risk*: the kill lands outside the intended step.
- *Mitigation*: each kill waits for the step's observable marker (API access log for fetch and post, a visible ffmpeg process for decode) before `ctl` acts.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The E-20 epic says the real-API tier runs in E-31 because E-21 has a higher id; this suite depends on E-21/c15, E-18 and E-22 tasks instead, as `tests.md` H2 requires a real API. The parent should confirm the ordering at Step 4 | Parent | open | Low |
