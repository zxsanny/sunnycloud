# FeedStamp: single-statement stamp and its concurrency suite

**Task**: AZ-3775_feed_stamp
**Name**: Feed stamp
**Description**: The one-statement stamp that increments an account's feed counter and writes the new `seq` to every touched node, gapless under concurrent commits and rollbacks
**Complexity**: 5 points
**Dependencies**: AZ-3774_sync_feed_skeleton
**Component**: 05 sync_feed
**Tracker**: AZ-3775
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c03

## Problem

Devices sync from a server-ordered feed (ADR-004): every tree change carries the account's next `seq`, and a device that reads from a cursor must see each change once and in commit order (AC-8, AC-69). The counter row is the one serialisation point of an account's commits (R07), so the stamp must be a single statement that runs last, and a rolled-back transaction must leave no gap (R2).

## Outcome

- `FeedStamp::stamp(tx, account, TouchedNodes)` is one data-modifying statement: it increments `account_feeds.seq` and writes the new value to every touched `nodes` row, and returns the `Seq`.
- A batch of up to 500 touched nodes shares one `seq`; untouched nodes keep their older `seq`.
- `TouchedNodes` cannot be empty; an account without a feed row returns `AccountFeedMissing` and logs ERROR.

## Scope

### Included
- The stamp, the non-empty touched set, the missing-row error, the concurrency suite and PT-05-1.

### Excluded
- Calling the stamp last inside 02's transactions (02's obligation, `feed-writer-ports.md`); reading the feed (E-12/c04); the "via 02" cases (E-12/c09).

## Acceptance Criteria

**AC-1: One stamp, one seq**
Given an account with a feed row (epoch E, seq 0) and 600 node rows
When one node is stamped, then 500 nodes in one call
Then the calls return 1 and 2, the 500 nodes all carry 2, untouched nodes keep their older value, and the statement log shows each stamp as one data-modifying statement

**AC-2: A rollback leaves no gap**
Given a stamp followed by ROLLBACK
When the next transaction stamps and commits
Then `account_feeds.seq` was unchanged by the rolled-back one and the committed stamp returns the next integer

**AC-3: Missing feed row is typed**
Given an account without a feed row
When `stamp` is called
Then it returns `AccountFeedMissing` (no panic) and one ERROR line with the account id is logged

**AC-4: The touched set is never empty**
Given an empty list of nodes
When a `TouchedNodes` value is built from it
Then construction fails with a typed error and no stamp can be issued

**AC-69: Concurrent commits get a gapless, once-only sequence (component part, R2)**
Given one account and 50 tasks per round released from a barrier, each writing a node fixture and stamping as its last statement, with 20% rolling back instead of committing, over 100 rounds
When all rounds finish
Then the committed stamps returned distinct, contiguous values, the final `account_feeds.seq` equals the number of committed stamps, the set of `nodes.seq` values equals 1..N, and no deadlock or serialization error reached a caller; the reader side is checked in E-12/c04, the path through 02 in E-12/c09, and the routed outcome by E-22/p06

**AC-5: Stamp throughput (PT-05-1)**
Given 50 writers on one account in a closed loop of stamp-last transactions for 2 minutes on REF-SRV
When the run finishes
Then throughput is at least 1,000 stamps/s (failure below 300), p95 at most 50 ms (failure 150 ms), p99 at most 100 ms (failure 300 ms), and gaps and duplicates are 0

## Non-Functional Requirements

**Performance**
- As AC-5; one database connection per writer.

**Reliability**
- 0 gaps or duplicates under any mix of commits and rollbacks.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | `TouchedNodes` construction | Empty input refused |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Feed row, 600 nodes (fixture SQL as `sunnycloud_migrator`) | IT-05-1 stamp cases | 1, then 2 shared by 500; one statement | AC-8 |
| AC-2 | Stamp then rollback | IT-05-1 rollback case | No gap | |
| AC-3 | No feed row | IT-05-1 missing-row case | `AccountFeedMissing`, ERROR | |
| AC-69 | 50 tasks x 100 rounds, 20% rollback | IT-05-2 writer part | Set of `nodes.seq` == 1..N; 0 deadlocks | NFT-RES-35 |
| AC-5 | 50 writers, 2 min | PT-05-1 | >= 1,000/s; p95 <= 50 ms; 0 gaps | NFT-PERF-09 |

## Constraints

- The stamp is the only writer of `nodes.seq`; node rows in tests come from fixture SQL run as `sunnycloud_migrator`.
- PT-05-1 timing on REF-SRV only; elsewhere `NOT-RUN(HW-REF)`.

## Risks & Mitigation

**Risk 1: Hot counter row (R07)**
- *Risk*: one account's commits queue on its feed row.
- *Mitigation*: the stamp is last, so the lock is held only from the stamp to COMMIT; measured by PT-05-1 and NFT-PERF-09.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |

## Contract

This task produces/implements the `FeedStamp` part of the contract at `_docs/02_document/contracts/05_sync_feed/feed-writer-ports.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
