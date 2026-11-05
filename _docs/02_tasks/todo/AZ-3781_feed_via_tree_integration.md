# Sync feed integration through the real TreeCommit ("via 02")

**Task**: AZ-3781_feed_via_tree_integration
**Name**: Feed via tree integration
**Description**: Repeat the feed's ordering and wake cases through 02's real `publish`: 50 concurrent commits seen once and in order, stamp last and notify after COMMIT, and a commit on device A waking device B
**Complexity**: 2 points
**Dependencies**: AZ-3775_feed_stamp, AZ-3776_change_feed_read, AZ-3777_feed_long_poll, AZ-3709_tree_publish_core
**Component**: 05 sync_feed
**Tracker**: AZ-3781
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c09
**Document dependencies**: `_docs/02_document/contracts/05_sync_feed/feed-writer-ports.md`

## Problem

The feed's own tests stamp from fixture SQL. Only the real commit path of 02 shows that the stamp is the last statement, that notify fires only after COMMIT, and that revived ancestors are in the touched set. The epic row is 1 point; AT-05-1 is moved here from E-12/c08 because its first step publishes through 02, so the task is 2 points.

## Outcome

- IT-02-13 repeated from the feed side: 50 concurrent `publish` calls on one account read back from cursor 0 exactly once and in order (AC-69).
- The stamp-last and notify-after-COMMIT obligations of `feed-writer-ports.md` checked against the real `publish`.
- AT-05-1: a publish on device A wakes device B's held read within 50 ms.

## Scope

### Included
- The three cases above with the real 02 `TreeCommit`, the real feed and real PostgreSQL 18.

### Excluded
- The same outcomes through the HTTP routes (E-22/p06); 02's own commit tests (E-13/c05).

## Acceptance Criteria

**AC-69: Concurrent commits through 02 are seen once and in order (component "via 02" part, R2)**
Given one account and 50 concurrent `publish` calls on distinct files through the real `TreeCommit`
When a reader reads the feed from `E.0` to head
Then every committed node appears exactly once in its latest state, the distinct `seq` values are 1..N for N committed publishes with none skipped or repeated, and the final cursor equals head (IT-02-13 repeated); the routed outcome is checked by E-22/p06

**AC-1: Stamp last, notify after COMMIT**
Given a real `publish` with a held reader on the account, and a second `publish` that is rolled back after its stamp
When both finish
Then the statement log shows the stamp as the last data-modifying statement before COMMIT, the reader wakes after the first commit only, and the rolled-back publish leaves no gap and wakes nobody

**AC-2: A commit on A wakes B (AT-05-1)**
Given one account and device B parked at head with `wait = 300 ms`
When device A publishes a file through 02
Then the counter advances by one, the publish commits and notifies, and B's read returns a page with the new node at the new `seq` within 50 ms

## Non-Functional Requirements

**Performance**
- Wake within 50 ms server side (AC-5 share).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (integration suite) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-69 | 50 concurrent publishes | IT-02-13 repeated from the feed side | Each node once; seq 1..N | NFT-RES-35, FT-P-11 |
| AC-1 | Real publish; rolled-back publish | Statement log and holder | Stamp last; wake after COMMIT only | |
| AC-2 | B parked at head | AT-05-1 | Page within 50 ms | AC-5, FT-P-07 |

## Constraints

- No component is faked: real 02 `TreeCommit` and its ports as 02's own integration tests wire them.

## Risks & Mitigation

**Risk 1: Ancestors missing from the touched set**
- *Risk*: a revived ancestor folder is not stamped and clients never re-list it.
- *Mitigation*: the contract requires 02 to pass revived ancestors; E-13/c06 tests it, and AC-69 here reads the latest state of every node.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| These cases need the real 02 crate from 05's tests; the E-00 crate-graph test forbids edges between component crates and does not say whether dev-dependencies count; otherwise the cases must live in the composition root's tests (owned by E-22) | E-00 / E-22 | open | Medium |
