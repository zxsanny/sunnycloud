# Media integration suite IT-08-1..20 on harness H1

**Task**: AZ-3847_media_integration_suite
**Name**: Media integration suite
**Description**: Build harness H1 and run every IT-08 case against real PostgreSQL, the real tree, transfer and blobstore, and a real HTTP server on the worker listener, in both the port form and the wire form
**Complexity**: 5 points
**Dependencies**: AZ-3837_media_shared_params_protocol, AZ-3838_media_policy, AZ-3840_job_queue, AZ-3841_job_planner_enqueue_effect, AZ-3842_derived_store_accept_input, AZ-3843_worker_auth_jobs_facade, AZ-3844_thumbnail_states_derived_opener, AZ-3845_media_janitors_erase_hook, AZ-3846_media_admin_health_doctor, AZ-3801_crate_skeleton_accept_loop, AZ-3806_auth_extraction, AZ-3831_internal_router_test_clock, AZ-3738_transfer_commit_upload, AZ-3730_gc_sweeper, AZ-3858_decoder_probe, AZ-3863_decoder_media_build_stages, AZ-3864_decoder_image_build_checks
**Component**: 08 media_jobs
**Tracker**: AZ-3847
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — wire form under test (E-18/c02).
- `_docs/02_document/contracts/08_media_jobs/media_policy.md` — policy under test on real probes (E-18/c03).
- `_docs/02_document/contracts/08_media_jobs/media_enqueue_effect.md` — commit effect under test (E-18/c06).
- `_docs/02_document/contracts/08_media_jobs/derived_access.md` — thumbnail state and derived lookup under test (E-18/c09).
- `_docs/02_document/contracts/08_media_jobs/maintenance_ports.md` — erase hook, cleanup and janitors under test (E-18/c10).
- `_docs/02_document/contracts/08_media_jobs/media_admin_health.md` — admin, doctor and metrics under test (E-18/c11).

## Problem

Each 08 task proves its port against PostgreSQL. The worker routes are the wire form of `WorkerJobs`, and several cases need other real components: uploads through 03 (IT-08-12), 04's GC (IT-08-10), the real decoder's probes (IT-08-17) and the HTTP layer on `:8081` (IT-08-4, 6, 9 to 12, 16, 20). Without one harness running all of them together, drift between the port and the wire shows first in the pipelines.

## Outcome

- Harness H1 (08 tests.md): a Rust integration binary with real PostgreSQL 18 and both roles, real 02, 03 and 04 (temp XFS blob directory, per-run master key), the injectable clock, a real HTTP server on `:8081`, and the short settings (`job_lease` 2 s, `job_attempts` 3, backoff 200 ms, `bulk_queue_threshold` 5, `job_terminal_ttl` 3 s, caps 64 KiB, 256 KiB, 2 MiB).
- IT-08-1 to IT-08-20 pass, each in its port form and, for the worker operations, its wire form.
- IT-08-19 is no longer BLOCKED (G-08-1 resolved).

## Scope

### Included
- H1, its data factories (real queue functions; direct inserts only for states that cannot be reached quickly: dead, exhausted), and the 20 cases.

### Excluded
- Performance, security and acceptance tests (E-18/c13); the real supervisor (E-20, AZ-3661).

## Acceptance Criteria

**AC-1: Wire form of the worker operations**
Given H1 with the worker secret
When the IT-08-4, 6, 8, 9, 10, 11, 12 and 20 cases run over HTTP
Then claims answer 200 with a job or 204 with no body, lease 200 or 204 or 409 `lease_lost`, input 200 with the exact length and `Cache-Control: no-store` or 404 `revision_obsolete` or 409, outputs 204 or 200 or 413 or 422 or 409 or 404, result 204, failure 200 with `{result}`, and status 204

**AC-2: Uniform 403 on the wire (AC-79)**
Given no header, an empty bearer, a wrong secret, a user session token, a device token and a session cookie
When each calls `claims`, `input` and `status`
Then every answer is 403 with the same status, body and headers once `X-Request-Id` and `Date` are removed, and a valid secret on a job it does not hold gets 409, never 403

**AC-3: Policy on real probes (AC-11)**
Given probe output of the real decoder image for DS-07, the seven DS-35 edge clips and two controls
When `decide` runs on each
Then DS-07 is `SkipCompliant`, the six near-miss clips are `Transcode`, the index-at-end clip is `Remux`, and both controls are `SkipCompliant`

**AC-4: Every IT-08 case passes**
Given H1
When IT-08-1 to IT-08-20 run
Then all pass with no fake store or in-memory queue anywhere

**AC-5: Coverage**
Given the suite and the task-level tests
When coverage is measured
Then `accept_output`, `claim` and worker authentication are at least 90% and the crate at least 75%

## Non-Functional Requirements

**Performance**
- Whole suite within the sum of the IT-08 "max execution time" values.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None beyond the harness factories | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | H1, worker secret | IT-08-4, 6, 8, 9, 10, 11, 12, 20 over HTTP | Statuses as listed | NFT-SEC-50 |
| AC-2 | Six credential kinds | IT-08-16 wire form | Identical 403; 409 for non-holder | NFT-SEC-50 |
| AC-3 | DS-07, DS-35 probes | IT-08-17 real-probe part | Skip, transcode, remux as listed | FT-P-25, NFT-PERF-17 |
| AC-4 | H1 | IT-08-1 to IT-08-20 | All pass | FT-P-20..27 |
| AC-5 | Suite | Coverage report | 90% critical; 75% overall | |

## Constraints

- No codec is stubbed; real `ffprobe` comes from the decoder image (08 tests.md setup step 3).
- Database per test from a template; blob directory per test; claim tests use unique accounts.

## Risks & Mitigation

**Risk 1: Harness depends on later epics**
- *Risk*: the HTTP part needs E-21's server and internal router.
- *Mitigation*: port-form cases run as soon as the 08 tasks land; the wire form joins when E-21/c15 is done.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| H1's wire form needs E-21/c03, c07 and c15 and the decoder image (E-19/c14), which ties this suite to three other epics | E-18 owner | open | Low |
