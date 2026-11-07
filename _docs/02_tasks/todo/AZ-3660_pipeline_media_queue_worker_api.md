# Media pipeline stage 1: commit to job and worker listener (F10)

**Task**: AZ-3660_pipeline_media_queue_worker_api
**Name**: Media pipeline stage 1
**Description**: A media commit enqueues jobs in the commit transaction, and the worker listener serves claim, lease, input fetch and failure to an authenticated supervisor only, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3741_docker_compose_skeletons, AZ-3801_crate_skeleton_accept_loop, AZ-3806_auth_extraction, AZ-3831_internal_router_test_clock, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3805_media_commands, AZ-3840_job_queue, AZ-3841_job_planner_enqueue_effect, AZ-3843_worker_auth_jobs_facade, AZ-3845_media_janitors_erase_hook, AZ-3738_transfer_commit_upload, AZ-3728_blob_reader, AZ-3707_tree_reader, AZ-3655_pipeline_large_upload, AZ-3652_initial_structure; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3660
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p08

## Problem

F10 starts inside the commit: 08's `MediaEnqueueEffect` must be in the effects list 12 gives 03, and the worker routes must sit on the internal listener `:8081` behind 08's `WorkerAuthenticator`, never on the public listener. Both are composition decisions made in `serve`. No planned task proves through the real server that a media commit yields a job, that only a worker bearer reaches the worker routes, and that the fetched input is the plaintext.

## Outcome

- A photo or video commit through the real `serve` process creates its jobs in the same transaction; other files create none.
- The worker listener serves claim, lease, input, skip and fail to a worker bearer and refuses every user credential; the public listener and Caddy expose no worker route.
- Every component operation of this stage has a production caller; at least one integration test runs it over HTTP against the real binary.

## Scope

### Included
- Flow order and seams (`system-flows.md` F10 steps 1-3, the API side):
  1. Commit (E-22/p03, p04) → `CommitEffect` list built by 12 includes `MediaEnqueueEffect` → `JobPlanner` → job rows (interactive or bulk lane) in the commit transaction.
  2. Internal listener `:8081` (bound to the backend address only) → `WorkerAuthenticator::authenticate(bearer)` → `WorkerJobs::{claim, extend_lease, release, complete_skipped, fail, record_status}`.
  3. `WorkerJobs::open_input` → `TreeReader::get_revision`, `is_current_or_retained` → `BlobReader` decrypted stream.
  4. Scheduler (12) → `JobReaper::run_cycle` returns expired leases; three attempts then failed.
- In this stage the test plays the supervisor's HTTP role with the configured worker bearer; the real supervisor and decoders run in stage 2 (E-22/p09).
- The end-to-end integration test of the steps above.

### Excluded
- Decoding, outputs and derived serving (E-22/p09), the supervisor's own loop (E-20), transcode policy (E-18/c03).

## Acceptance Criteria

AC-1..AC-4 are local to this task. AC-72 (fetch part) and AC-79 keep their numbers from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Jobs are born in the commit**
Given a signed-in device
When it commits a JPEG, a text file, and a JPEG whose commit is rolled back by a refused entry
Then the JPEG has its job rows on the interactive lane (`admin media-stats` shows them), the text file has none, and the rolled-back commit left none

**AC-2: Claim and lease over the worker listener**
Given a queued job and the worker bearer
When the job is claimed, the lease is extended, then the claimant goes silent and the test clock passes the lease
Then the claim returns the job, the extension succeeds, and the scheduler's job reaper returns the job to the queue with attempt + 1; after the third expiry the job is failed

**AC-3: Obsolete input**
Given a queued job whose file is deleted and whose revision is already collected (retention expired on the test clock) before the fetch
When the input is requested
Then the listener answers 404 `revision_obsolete` and the job is closed with outcome `obsolete`; had the deleted revision still been retained, the input would be served

**AC-4: Every stage operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds `MediaEnqueueEffect` in the commit effects list built by 12, production callers of `WorkerAuthenticator::authenticate` and `WorkerJobs::{claim, extend_lease, release, complete_skipped, fail, open_input, record_status}` (internal routes), a scheduler registration of `JobReaper`, and the internal listener bound only to the backend address

### Interaction criteria (Step 1.8)

**AC-72 (fetch part): Worker input equals the plaintext** (R5)
Given a committed 1 GiB file and the worker bearer
When its job input is fetched over the worker listener
Then the BLAKE3 of the streamed bytes equals the file's plaintext hash, and the stream is verified block by block as the supervisor does (2 MiB blocks rolled up)
(The key-check part of AC-72 is owned by E-22/p13.)

**AC-79: Worker endpoints are closed to users and to the public route** (R12)
Given a user session, a device token and the test stack with the release Caddyfile
When the session and the device token call worker endpoints on `:8081`, and any worker route is requested through Caddy and on the public listener `:8080`
Then `:8081` answers 403 for both credentials, and Caddy and `:8080` answer 404 for every worker route

## Non-Functional Requirements

**Performance**
- Commit-to-claimable within 1 s for an interactive thumbnail job (part of AC-10's 5 s budget).

**Reliability**
- The worker has no database login; every queue change goes through `:8081`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Static caller check over non-test sources | Effect in the list; each named operation has a production caller; listener bound to backend only |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in device | Commit JPEG, text file, rolled-back JPEG | Jobs only for the committed JPEG | AC-10 |
| AC-2 | Queued job, worker bearer, test clock | Claim, extend, expire 3 times | Requeued with attempt + 1, then failed | AC-71 |
| AC-3 | Job, file deleted and revision collected | Fetch input | 404, job `obsolete` | FT-N-49 |
| AC-72 | Committed 1 GiB file | Fetch over `:8081` | Hash equals plaintext | FT-N-43 |
| AC-79 | Session, device token, Caddy | Worker routes on `:8081`, `:8080`, Caddy | 403 on `:8081`; 404 on public paths | NFT-SEC-50 |

## Constraints

- Owner: component 12 (`src/server/`): effect list, listener and janitor bindings for this stage, and its end-to-end tests among 12's integration tests. No SQL or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume; the Caddy check runs in the E-31/c05 test stack. The test acts as the HTTP caller in the supervisor's place; no sunnycloud server component is faked.
- Downstream: E-31/c26 (FT-P-20..27), E-31/c33 (FT-N-43) and E-31/c38 (NFT-SEC-50) should depend on this task.

## Risks & Mitigation

**Risk 1: Worker routes leak onto the public router**
- *Risk*: a shared route registry mounts worker routes on both listeners.
- *Mitigation*: AC-79 probes both listeners and Caddy.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `Range` on the worker input route is ignored: always 200, full body (G-08-4, decided 2026-11-07) | E-18 | resolved | Low |
