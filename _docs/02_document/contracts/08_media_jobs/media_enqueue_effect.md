# Contract: MediaEnqueueEffect (job planning inside the commit transaction)

**Component**: 08_media_jobs
**Producer task**: E-18/c06 — `_docs/02_tasks/todo/B05-17_job_planner_enqueue_effect.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-14/c08 (passes the effect to `publish`), E-14/c15 and E-13/c16 (integration passes with the real effect), E-22/c03 (effects list); pipeline AZ-3660
**Version**: 1.0.0
**Status**: draft (open point 1 owned by E-13)
**Last Updated**: 2026-11-04

## Purpose

What 08's `CommitEffect` does inside 02's commit transaction, so the tree and transfer owners know its cost and its rollback behaviour. The protocol itself (savepoints, order, the stamp-last rule) is `_docs/02_document/contracts/02_tree/tree_commit.md`.

## Shape

| Hook | Behaviour | Result |
|------|-----------|--------|
| `on_begin(tx)` | nothing | `Proceed` |
| `on_entry(tx, &PublishedEntry)` | plan; for each planned job skip when every output has a derived row for (account, content hash, kind, params version), else insert ignoring a conflict on the job key; choose priority | `Ok(())`; a database failure is returned as the effect's error under the tree-commit rules |
| `on_batch(tx, &[EntryOutcome])` | nothing | `Ok(())` |

### Planning

| Entry | Jobs |
|-------|------|
| photo (see `media_policy.md` supported kinds) | one `stills` |
| video | one `poster`, one `transcode` |
| `vault = true`, SVG, unknown or unsupported type, no media type, size 0 | none |

### Priority

| Condition | Priority |
|-----------|----------|
| `import_hint` set on the entry | 1 (bulk) |
| the account already has more than `bulk_queue_threshold` queued jobs (counted with limit threshold + 1 on the queued-jobs index) | 1 (bulk) |
| otherwise | 0 (interactive) |

## Invariants

- No I/O; reads only 08's own rows; at most 2 indexed probes and 1 insert per planned job.
- A job row exists only if its entry's savepoint commits: a failed entry or a rolled-back commit leaves no job.
- Enqueue is idempotent on (account, kind, content hash, params version); the same content in one account is planned once per params version, whatever the number of revisions or nodes.
- The effect also runs for `NoChange` entries (tree-commit rule); the idempotent enqueue and the derived-row skip make that safe.

## Non-Goals

- Not covered: claiming and running jobs (`worker_job_protocol.md`).

## Versioning Rules

- **Breaking changes** (more statements per entry, a new error path, a change of the priority rule) require a new major version and notice to E-13 and E-14.
- **Non-breaking additions** (a new supported type) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| matrix | 12 media types incl. vault and zero-byte | jobs per the planning table | IT-08-1 |
| idempotent | same entry twice; partial and full derived rows | one job; skip rules | IT-08-2 |
| bulk | `import_hint`; threshold 5 with 5 and 6 queued | priorities 1, 0, 1 | IT-08-3 |
| media-job-rollback | failing entry after the effect ran | no job for it | tree-commit test case |
| statement-count | one photo entry | 2 probes + 1 insert | IT-08-1 |

## Open points

1. `PublishedEntry` has no `account_id` (contracts part 2 open point 3); E-13 decides whether it gains one or the effect reads it from `CommitActor`.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
