# Contract: tree-commit (TreeCommit::publish and the CommitEffect protocol)

**Component**: 02_tree
**Producer task**: E-13/c05 — `_docs/02_tasks/todo/B03-05_tree_publish_core.md` (contributors: E-13/c06 conflict outcomes, E-13/c07 usage deltas, E-13/c08 blob ownership; contract-test plan from E-13/c01); keys assigned at Step 4
**Consumer tasks**: E-14/c08, E-14/c09, E-14/c12, E-14/c15, E-13/c16 (this batch); E-18/c06 (08 media effect), E-22/c03 (composition root), resolved at Step 4
**Version**: 1.0.0
**Status**: draft (frozen when the producer task passes review and E-13/c01 has completed the Test Cases)
**Last Updated**: 2026-11-04

## Purpose

The one transaction that publishes file content to the tree, and the protocol by which 03 and 08 add their writes to it without opening it (ADR-015). Signatures and DTOs live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 02 tree and § Commit-effect protocol); this file freezes the behaviour.

## Shape

Signatures: part 2 (`TreeCommit::publish`, `PublishEntry`, `Target`, `NewRevision`, `EntryResult`, `PublishOutcome`, `CommitEffect`, `EffectError`, `PublishedEntry`, `EntryOutcome`).

### Sequence inside one transaction

| Step | What happens | On failure |
|---|---|---|
| 1 | `AccountGuard::ensure_active(tx, account)` | `AccountNotActive` for the whole call; nothing written |
| 2 | Account advisory lock | — |
| 3 | Every effect's `on_begin` | `Replay(response)`: return `Replayed`, nothing else runs; `Batch(code)`: whole rollback |
| 4 | Per entry, in its own savepoint: blob ownership check, node and revision writes, `QuotaLedger::adjust_used(delta)`, `QuotaLedger::release_reservation(reserved_bytes)`, `GcQueue::schedule_in_tx` for a replaced blob, then every effect's `on_entry` | typed entry failure or `Entry(code)`: that savepoint rolls back (its delta, release and effect writes included); other entries continue |
| 5 | Every effect's `on_batch` with all entry outcomes, failed ones included | `Batch(code)`: whole rollback, no seq |
| 6 | `FeedStamp::stamp` with the union of touched nodes (one seq per call) — the last statement; skipped when nothing was touched | — |
| 7 | COMMIT, then `FeedNotifier::notify` (skipped when no stamp) | — |

### Outcomes and usage deltas

| Outcome | When | Tree effect | Usage delta |
|---|---|---|---|
| `Created` | `New`, name free | new node and revision | +size |
| `Updated` / `Replaced` | `Existing`, base current (difference: open item) | new current revision; old row kept; old blob queued `replaced`, due now + retention | +new − old |
| `ConflictCopied{copy_node}` | `Existing`, base stale | new revision current; superseded revision row re-parented to a sibling with the conflict-copy name (no byte copy); both nodes touched | +new |
| `CreatedAsCopy{node}` | `New`, name taken | new file gets the conflict-copy name; never `name_conflict` | +size |
| `Revived` | `Existing` on a tombstoned file | node and tombstoned ancestors revived, `cascade_pending` cleared on them; conflict-copy name if the name was taken | +new |
| `NoChange{existing_revision}` | same content as current | nothing; effects still run | 0 |

Conflict-copy names come from helper 06 (`stem (conflict copy YYYY-MM-DD HH-MM device).ext`, UTC from the injected clock, `device` = `CommitActor.device_label`, 255-byte limit, ` (2)` suffix).

Per-entry failures: `NameInvalid`, `ParentDeleted`, `ParentNotFound`, `NotAFolder`, `NotFound`, `BlocklistMismatch` (blob not owned by the account; also for an unknown blob, same value), `EffectFailed(code)`. Whole-call: `AccountNotActive`, `CommitIdReused` (from an effect), `EffectFailed`.

## Invariants

- Nothing runs between the stamp and COMMIT (AC-69); concurrent commits on one account leave no gap or duplicate seq.
- The advisory lock is held while `on_begin` runs, so two identical receipt batches cannot both apply.
- An existing file is addressed by node id, never by path; a concurrent folder move changes only the parent (AC-70).
- A revision row exists only in the transaction where the blob effect flips the blob to committed (tree half of AC-68).
- A revision's `blob_id` belongs to the committing account.
- Effects do no I/O, read only rows they own, and add no statement after the stamp.
- Usage after the call equals the sum of live current revisions (AC-74).

## Non-Goals

- Not covered: upload sessions, receipts, sniffing (03, `../03_transfer/commit_service.md`); job planning (08).
- Not covered: wire responses (11, and G-03-3 in 03's commit contract).

## Versioning Rules

- **Breaking changes** (sequence order, a new `PublishOutcome` variant, a changed `EffectError` meaning) require a new major version, a `contracts` change task in E-13, and notice to E-14 and E-18.
- **Non-breaking additions** (new optional `PublishedEntry` field) require a minor version bump.

## Test Cases

Contract tests to be completed by E-13/c01 (owning task in brackets).

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| order-and-tail | normal batch with statement log | guard < lock < `on_begin` < savepoints < `on_batch` < stamp < COMMIT; 0 statements after stamp | IT-02-10 [E-13/c05], IT-03-19 [E-14/c12] |
| replay-under-lock | identical receipt batch twice, also concurrently | second is `Replayed`; exactly one applies | IT-02-10 [E-13/c05], IT-03-8 [E-14/c09] |
| entry-failure-rollback | `Entry(UploadNotFound)` after release | that entry's release and writes rolled back; others commit; one seq | IT-02-9 [E-13/c05], IT-03-9 [E-14/c08, E-14/c11] |
| batch-failure | `on_batch` returns `Batch` | full rollback; no seq | IT-02-10 [E-13/c05] |
| reaper-vs-commit | reaper racing commit on one session | exactly one winner; reservation released once | IT-03-9 [E-14/c11] |
| media-job-rollback | failed entry with the real media effect | no job row for it | [E-13/c16, E-14/c15, E-18/c06] |
| foreign-blob | entry naming another account's blob | typed entry failure equal to a random id | ST-02-4 [E-13/c08] |

## Open items

- `Updated` versus `Replaced`: the difference is not defined (02 §2).
- Which of `BlocklistMismatch` and `NotFound` is the single foreign/unknown blob answer (02 risks (c)).
- Part 2 open point 3: 08's job key needs the account id, which `PublishedEntry` lacks; E-13 decides whether `PublishedEntry` gains `account_id` or effects take it from `CommitActor`.
- Part 2 open point 4: `StoredResponse` must be 03's serialised `BatchResult` (see `../03_transfer/commit_service.md`).
- Reviving an ancestor before the cascade has marked its descendants clears `cascade_pending`, so unmarked descendants come back live (E-13/c06).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
