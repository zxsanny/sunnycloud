# JobPlanner and the media commit effect

**Task**: AZ-3841_job_planner_enqueue_effect
**Name**: Job planning at commit
**Description**: Inside 02's commit transaction, plan the thumbnail and transcode jobs a newly published file needs, choose interactive or bulk priority, skip work whose outputs already exist, and enqueue idempotently without any I/O
**Complexity**: 3 points
**Dependencies**: AZ-3838_media_policy, AZ-3840_job_queue, AZ-3769_contracts_part2, AZ-3709_tree_publish_core
**Component**: 08 media_jobs
**Tracker**: AZ-3841
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — the `CommitEffect` protocol and its rules (E-13/c05).
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `CommitEffect`, `PublishedEntry`, `EffectError` (E-00/c04).
- `_docs/02_document/contracts/08_media_jobs/media_policy.md` — `supported_kinds_for` (E-18/c03).

## Problem

A job must exist exactly when its revision is committed: born in the same transaction, rolled back with it (AC-71). Photos must be queued for the interactive lane unless they are part of a bulk import (AC-10), and the same content must not be processed twice for one account.

## Outcome

- `JobPlanner::plan` (pure): a photo gives one `stills` job; a video gives one `poster` and one `transcode` job; vault files, SVG, unknown and unsupported types and zero-byte files give none.
- A planned job is skipped when every one of its outputs already has a derived row for (account, content hash, kind, params version); otherwise it is enqueued idempotently.
- Priority 1 (bulk) when the entry carries `import_hint` or the account already has more than `bulk_queue_threshold` queued jobs (counted with a limit of threshold + 1); otherwise 0.
- `MediaEnqueueEffect::on_entry` runs this inside the commit transaction; `on_batch` does nothing; the effect issues no I/O and no read beyond the indexed probes.

## Scope

### Included
- The planner, the priority rule, the derived-row skip and the effect.
- The contract file `contracts/08_media_jobs/media_enqueue_effect.md`.

### Excluded
- Supplying the effect to `publish` (03, E-14/c08) and the effects list (E-22/c03).
- The real commit path end to end (E-13/c16, E-14/c15, AZ-3660).

## Acceptance Criteria

**AC-1: Planning matrix (AC-10, AC-11)**
Given published entries for JPEG, PNG, WebP, GIF, HEIC, MP4, MOV, MKV, SVG, `application/octet-stream`, a zero-byte file and a vault JPEG, each in its own transaction
When the effect runs
Then each photo type has one `stills` job on lane `image`, each video type one `poster` and one `transcode` job on lane `video`, the others none, and `on_batch` adds none

**AC-2: Idempotent and skips existing outputs (AC-71)**
Given one entry planned twice, a second revision with the same content hash, an entry whose `thumb` derived row exists but `preview` does not, and an entry with both
When the effect runs for each
Then one job exists after the first two, the partial case enqueues the `stills` job, the full case enqueues nothing, and a new `params_version` enqueues a new job for the same content

**AC-3: Bulk priority (AC-10)**
Given entries with `import_hint`, an account with 5 queued jobs (threshold 5) receiving a 6th and 7th entry, and another account with 1 queued job
When the effect runs
Then import entries get priority 1, the entry planned with 6 jobs already queued gets 1 while the one planned with 5 queued gets 0, the other account's job gets 0, and the counting probe uses a limit of 6

**AC-4: Rolled back with its entry**
Given a publish where an entry fails after the media effect ran in that entry's savepoint
When the commit ends
Then no job row exists for the failed entry, and the committed entries have theirs

**AC-5: No I/O, bounded statements**
Given one photo entry
When the effect runs
Then it issues at most 2 indexed probes and 1 insert per planned job, and no file or network access

## Non-Functional Requirements

**Performance**
- Commit-to-claimable within 1 s for an interactive job (AZ-3660 NFR).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Planner per media type, vault flag, zero size | As listed |
| AC-3 | Priority rule at threshold, threshold + 1 | 0 then 1 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Entries of 12 types | IT-08-1 | Jobs per matrix | FT-P-20, FT-P-39 |
| AC-2 | Repeat, same hash, partial, full | IT-08-2 | One job; skip rules | NFT-RES-36 |
| AC-3 | Import entries; threshold 5 | IT-08-3 | Priorities as listed | FT-P-21, NFT-PERF-14 |
| AC-4 | Failing entry after the effect | `media-job-rollback` case of the tree-commit contract | No job for the failed entry | AC-71 |
| AC-5 | One photo entry | IT-08-1 statement count (`pg_stat_statements`) | 2 probes + 1 insert | |

## Constraints

- The effect follows the tree-commit protocol: no I/O, reads only 08's own rows, adds no statement after the feed stamp.

## Risks & Mitigation

**Risk 1: Effect slows every commit**
- *Risk*: the probes add latency to small-file commits (AC-40).
- *Mitigation*: index probes only, bounded by AC-5.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `PublishedEntry` carries no `account_id` (contracts part 2 open point 3); the job key needs it. E-13 decides whether the entry gains it or the effect takes it from `CommitActor` | E-13 owner | open | Medium |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/media_enqueue_effect.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
