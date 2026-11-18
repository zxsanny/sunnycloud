# Remediate: media jobs outlive a collected revision

**Task**: AZ-4332_remediate_media_jobs_obsolete
**Name**: Obsolete media jobs
**Description**: Make a media job whose revision is collected answer 404 `revision_obsolete` on input and output and end with outcome `obsolete`, as `worker_job_protocol.md` states, instead of being deleted with the revision
**Complexity**: 3 points
**Dependencies**: AZ-3842_derived_store_accept_input
**Component**: 08 media_jobs
**Tracker**: AZ-4332
**Epic**: AZ-3629 (E-18)
**Ref**: remediation of AZ-3660 AC-3 (owner decision 2026-11-18)

## Problem

`jobs` references `revisions (id, account_id)` with `ON DELETE CASCADE` (migration `20261106160000_create_job_tables`). When the GC collects a revision, its jobs disappear with it, so a worker holding the lease gets 409 `lease_lost` on input, and nothing can close the job `obsolete`. The contract (`_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md`, input and output rows, rule after the table, scenario obsolete-input, IT-08-12) says 404 `revision_obsolete`, then the supervisor closes the job through `result` with outcome `obsolete`. The owner decided on 2026-11-18 that the contract wins.

## Outcome

- A job keeps its row after its revision is collected; input and output of that job answer 404 `revision_obsolete`; `result` with `obsolete` closes it.
- Account erase still leaves no job row of the account (08's erase hook), and no job is left without an end state.

## Scope

### Included
- A new migration changing the `jobs` revision reference (for example `ON DELETE SET NULL (revision_id)`, or no foreign key with the obsolete check in 08), and the 08 code paths that read the revision (claim, input, output, result, reaper, erase hook).
- 08 component tests for the collected case; un-ignore `ac3_the_input_of_a_collected_revision_is_404_revision_obsolete_and_the_job_closes_obsolete` in `src/server/tests/pipeline_media_worker.rs`.

### Excluded
- Any contract change (the contract stays as is).

## Acceptance Criteria

**AC-1: Collected revision answers revision_obsolete**
Given a leased `stills` job whose file was deleted and whose revision the GC has collected
When the worker asks for the input, and then sends an output
Then both answer 404 `revision_obsolete`, and the job row still exists

**AC-2: Obsolete close**
Given that job
When the worker sends `result` with outcome `obsolete`
Then the answer is 204 and the job is closed with outcome `obsolete`

**AC-3: Erase leaves nothing**
Given an account with open jobs, some on collected revisions
When the account is erased
Then no job row of the account remains

**AC-4: Pipeline proof**
Given `src/server/tests/pipeline_media_worker.rs`
When the AC-3 collected-revision test runs un-ignored three times
Then it passes every time

## Constraints

- Migrations are additive (a new migration; the shipped one is not edited). No table or column rename.
