# Worker suite on H2: input fetch on the real API

**Task**: AZ-4387_worker_suite_input_fetch
**Name**: Worker suite, input fetch on the real API (H2)
**Description**: IT-09-7 and IT-09-6 (i) against the real API on the stack
**Complexity**: 3 points
**Dependencies**: AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl, AZ-4384_worker_suite_gate_decisions; decision D2 (`_docs/02_task_plans/az-4385-worker-faults/decisions.md`)
**Component**: 09_worker_supervisor
**Tracker**: AZ-4387
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-4385 in batch 131 (the IT-09-7 and IT-09-6 (i) parts of its AC-4)

## Acceptance Criteria

The expected results are in `_docs/02_document/components/09_worker_supervisor/tests.md`.

**AC-1: Input fetch verification and limits (IT-09-7)**
Given (a) a job whose content hash differs from the stored revision, (b) a 2 MiB photo with `worker_max_input_photo=1MiB`, (c) a body that grows past its `Content-Length`, (d) a clean photo, (e) an input larger than the free space of a small scratch mount
When each job is claimed and fetched
Then each meets its expected result (G-09-2: only "nothing posted, not left running" for (e))

**AC-2: A fetch that fails on the server is retried (IT-09-6 (i))**
Given a job whose input fetch answers 5xx
When the supervisor fetches it
Then the job fails with `Retry` and is queued again with its backoff

## Decision D2 (`_docs/02_task_plans/az-4385-worker-faults/decisions.md`)

- **D2a, applied**: leg (i), the fetch 5xx, comes from the real API through the listed fault injector "Bit flip at rest"
  (`bit-flip`, to implement in ctl): one byte of the input blob's first chunk flipped while the supervisor is stopped.
  Assert the API's 5xx on the input route, do not move the test clock, and run it in its own module.
- **D2b, open** (OPERATOR_TODO D2): leg (c) cannot happen with the real API (exact `Content-Length` by contract, and hyper
  trims a longer body). Recommended: NOT-RUN on the stack with a pointer to `src/worker/tests/api_input.rs:175`, and
  09 tests.md (c) reworded to the client-side case. Only leg (c) waits for the answer.

The other legs need harness work only: (a) a ctl action that edits a job row's hash (a write, so a new fault-injector
row), (b) a supervisor started with another photo limit, (e) a small scratch mount for one run.

The worker crate's tests already cover every IT-09-7 leg against a local peer: `src/worker/tests/api_input.rs:69` (hash),
`:108` (photo limit), `:175` (a body with no declared length past the limit), `src/worker/tests/scratch.rs:279` (free space).

## Constraints

- The real API, queue and PostgreSQL in every case; leg (c) as D2b decides.
