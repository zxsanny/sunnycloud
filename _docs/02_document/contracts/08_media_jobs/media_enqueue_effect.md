# Contract: MediaEnqueueEffect (job planning inside the commit transaction)

**Component**: 08_media_jobs
**Producer task**: E-18/c06 — AZ-3841 (`_docs/02_tasks/done/AZ-3841_job_planner_enqueue_effect.md`)
**Consumer tasks**: E-14/c08 (passes the effect to `publish`), E-14/c15 and E-13/c16 (integration passes with the real effect), E-22/c03 (effects list); pipeline AZ-3660
**Version**: 1.1.0
**Status**: implemented in `src/components/08_media_jobs/src/enqueue_effect.rs` and `planner.rs`
**Last Updated**: 2026-11-12

## Purpose

What 08's `CommitEffect` does inside 02's commit transaction, so the tree and transfer owners know its cost and its rollback behaviour. The protocol itself (savepoints, order, the stamp-last rule) is `_docs/02_document/contracts/02_tree/tree_commit.md`.

## Shape

| Name | Signature | Notes |
|------|-----------|-------|
| `JobPlanner::new` | `(params_version: u32, bulk_queue_threshold: u64) -> JobPlanner` | `Copy`, pure |
| `JobPlanner::from_config` | `(&MediaConfig) -> JobPlanner` | `CURRENT_PARAMS_VERSION` (media-shared) and `bulk_queue_threshold` (`SUNNYCLOUD_BULK_QUEUE_THRESHOLD`, default 200) |
| `JobPlanner::plan` | `(&self, account_id: Uuid, &PublishedEntry) -> Vec<JobSpec>` | pure |
| `JobPlanner::count_limit` | `(&self) -> u64` | threshold + 1 (saturating) |
| `JobPlanner::priority` | `(&self, import_hint: bool, queued: u64) -> Priority` | pure |
| `MediaEnqueueEffect::new` | `(Arc<JobQueue>, JobPlanner, account_id: Uuid) -> MediaEnqueueEffect` | built per `publish` call; `account_id` is `CommitActor.account_id` |

| Hook | Behaviour | Result |
|------|-----------|--------|
| `on_begin(tx)` | nothing | `Ok(Begin::Proceed)` |
| `on_entry(tx, &PublishedEntry)` | plan; when a job is planned, probe the `derived` kinds of (account, content hash, params version); drop each job whose outputs all exist; when a job remains, choose the priority (the queued-jobs count is probed only without `import_hint`); `JobQueue::enqueue_in_tx` per job (insert, `ON CONFLICT ON CONSTRAINT jobs_key DO NOTHING`) | `Ok(())`; a database failure is logged and returned as `EffectError::Batch(ErrorCode::Internal)` (whole rollback, tier 3) |
| `on_batch(tx, &[EntryOutcome])` | nothing | `Ok(())` |

### Planning

| Entry | Jobs (lane) | Outputs checked against `derived` |
|-------|------|------|
| photo: `image/jpeg`, `image/png`, `image/webp`, `image/gif`, `image/heic` (`media_policy.md` supported kinds, exact strings) | one `stills` (`image`) | `thumb`, `preview` |
| video: `video/mp4`, `video/quicktime`, `video/x-matroska` | one `poster` (`video`), one `transcode` (`video`) | `thumb`; `transcode` |
| `vault = true`, SVG, unknown or unsupported type, no media type, size 0 | none | — |

### Priority

| Condition | Priority |
|-----------|----------|
| `import_hint` set on the entry | 1 (bulk) |
| the account already has more than `bulk_queue_threshold` queued jobs (counted with `LIMIT threshold + 1` on `jobs_queued_idx`) | 1 (bulk) |
| otherwise | 0 (interactive) |

One priority per entry: a video's two jobs share it.

## Invariants

- No I/O; reads only 08's own rows (`derived`, `jobs`); per entry at most one `derived` probe (`derived_pkey` prefix), one queued-jobs count (`jobs_queued_idx`) and one insert per planned job — within 2 indexed probes and 1 insert per planned job. An entry with no planned job issues no statement.
- A job row exists only if its entry's savepoint commits: a failed entry or a rolled-back commit leaves no job.
- Enqueue is idempotent on (account, kind, content hash, params version); the same content in one account is planned once per params version, whatever the number of revisions or nodes.
- The effect also runs for `NoChange` entries (tree-commit rule); the idempotent enqueue and the derived-row skip make that safe.

## Non-Goals

- Not covered: claiming and running jobs (`worker_job_protocol.md`); building the effects list (E-22/c03).

## Versioning Rules

- **Breaking changes** (more statements per entry, a new error path, a change of the priority rule) require a new major version and notice to E-13 and E-14.
- **Non-breaking additions** (a new supported type) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| matrix | 12 media types incl. vault and zero-byte | jobs per the planning table; `on_batch` adds none | IT-08-1 (`effect_plan.rs`), planner unit tests |
| idempotent | same entry twice; second revision same hash; partial and full derived rows; params version 2 | one job; partial enqueues `stills`; full none; v2 a new job | IT-08-2 (`effect_skip.rs`) |
| bulk | `import_hint`; threshold 5 with 5 and 6 queued; other account with 1 | 1; 0 then 1; 0; probe limit 6 | IT-08-3 (`effect_priority.rs`), unit `the_counting_probe_limit_is_threshold_plus_one` |
| media-job-rollback | failing entry after the effect, in the real `publish`; `on_batch` failure | no job for it; others keep theirs; no job at all | `effect_rollback.rs` |
| statement-count | one photo entry | derived probe + queued count + insert, both probes on their index | IT-08-1, sqlx statement events (`pg_stat_statements` is not preloaded in the test database) |

## Open points

None. (Former point 1 decided 2026-11-12, AZ-3709: the account comes from `CommitActor`; `PublishedEntry` is unchanged.)

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.1.0 | 2026-11-12 | Implemented: constructor signatures, DB failure → `Batch(Internal)`, one priority per entry, count skipped with the hint, no statement without a planned job; open point 1 closed | AZ-3841 |
