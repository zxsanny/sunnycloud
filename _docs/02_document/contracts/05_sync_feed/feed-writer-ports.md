# Contract: feed-writer-ports

**Component**: 05_sync_feed
**Producer task**: E-12/c03 — `_docs/02_tasks/todo/B02-24_feed_stamp.md` (`FeedStamp`); E-12/c05 — `B02-26_feed_long_poll.md` (`FeedNotifier`); E-12/c06 — `B02-27_feed_horizon_lifecycle.md` (`FeedHorizon`) (renamed to their AZ keys at Step 4). The `nodes.seq` column and `nodes_feed_idx` come from E-12/c02 (`B02-23_sync_feed_skeleton.md`).
**Consumer tasks**: E-13 tree (commit tail in `publish`, folder create, rename, move and delete, edit-beats-delete revival, restore, tombstone purge; E-13/c04, c05, c06, c10, c11)
**Version**: 1.0.0
**Status**: draft (frozen when E-12/c06 is accepted)
**Last Updated**: 2026-11-04

## Purpose

The "documented contract with E-13" of the E-12 epic: how 02 stamps, wakes and raises the purge horizon so that the feed stays gapless and in commit order (AC-69, AC-8). Signatures are declared in `contracts` part 2 (E-00/c04). Source: `components/05_sync_feed/description.md` §2, `data_model.md` §7.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `FeedStamp::stamp` | `tx: &mut DbTx, account, TouchedNodes (non-empty)` -> `Seq` | `AccountFeedMissing` | async, caller's tx |
| `FeedNotifier::notify` | `account, seq` -> `()` | none | sync, non-blocking |
| `FeedHorizon::raise_purge_horizon` | `tx: &mut DbTx, account, seq` -> `()` | none | async, caller's tx |

## Invariants

- `stamp` is one data-modifying statement: it increments the account's counter and writes the new value to every touched node. A batch commit (up to 500 entries) shares one `seq`. A rolled-back transaction leaves no gap.
- `stamp` is the only writer of `nodes.seq`. The column (bigint, default 0) and the index `nodes_feed_idx (account_id, seq)` belong to 05 and are added to 02's `nodes` table by 05's migration, which sorts after 02's `nodes` migration. 02 never writes `seq`.
- `AccountFeedMissing` (no feed row for the account) is an invariant violation, logged at ERROR.
- `notify` never blocks and keeps no state for accounts without waiters. A lost or late wake is healed by the feed's safety recheck (`feed_safety_recheck`, 10 s); a change made by another process is seen the same way.
- `raise_purge_horizon` is a non-decreasing update in the caller's transaction (`SET purge_horizon = GREATEST(purge_horizon, $seq)`): a call with a lower `seq` leaves the horizon unchanged and is not an error (a lowered horizon would accept cursors whose tombstones were purged; a reset is `bump_epoch`). Zero matching rows (the feed row of an account being erased) is not an error.
- **Caller obligations (02)**:
  - call `stamp` as the **last** statement before COMMIT: nothing between it and COMMIT (no reads, no file I/O, no cycle checks);
  - call `stamp` only when at least one node changed; never for a transaction where every entry failed or was `NoChange`;
  - put every node whose feed-visible state changed into `TouchedNodes`, including ancestors revived by edit-beats-delete; do **not** stamp cascade-marked descendants (a folder tombstone stands for its whole subtree);
  - call `notify(account, seq)` only after COMMIT has succeeded;
  - call `raise_purge_horizon` as the last statement of the purge transaction, with the highest purged `seq`.

## Non-Goals

- Not covered: what a change means or which tombstones to purge (02); reading the feed (`change-feed.md`); cross-host fan-out (one API process in v1.0).

## Versioning Rules

- **Breaking changes** (stamp-last rule, the non-empty rule, who writes `nodes.seq`) require a new major version and notice to E-13.
- **Non-breaking additions** (the horizon non-decreasing rule, stated as a narrowing; decided 2026-11-07, G-05-3) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| stamp-one | 1 node, feed seq 0 | returns 1; node seq 1 | IT-05-1 |
| stamp-batch | 500 nodes | one shared seq | IT-05-1 |
| stamp-rollback | stamp then ROLLBACK, then stamp | counter unchanged, next stamp is the next integer | IT-05-1 |
| stamp-no-row | account without feed row | `AccountFeedMissing` | IT-05-1 |
| concurrent-50 | 50 concurrent stamping transactions, 20% rolled back | set of `nodes.seq` == 1..N | IT-05-2 |
| notify-after-commit | real publish; rolled-back publish | holders wake after the first only | E-12/c09 |
| horizon-zero-rows | raise for an erased account | `Ok` | IT-05-12 |
| horizon-lower-seq | horizon 30, raise with 20 | `Ok`, horizon stays 30 | IT-05-12 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
