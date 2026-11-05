# FeedHorizon and the feed's account lifecycle hook

**Task**: AZ-3778_feed_horizon_lifecycle
**Name**: Feed horizon and lifecycle hook
**Description**: Raise the purge horizon inside 02's purge transaction, create each account's feed row with the installation epoch, and delete it at account erase
**Complexity**: 2 points
**Dependencies**: AZ-3773_feed_gaps_spike, AZ-3774_sync_feed_skeleton, AZ-3777_feed_long_poll, AZ-3746_blob_maintenance_ports
**Component**: 05 sync_feed
**Tracker**: AZ-3778
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c06
**Document dependencies**: `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` (`EpochMarker`, produced by E-10/c13)

## Problem

After tombstones are purged, a device whose cursor is older than the purge must resync instead of missing deletions (AC-41). Every account needs a feed row from creation, all with the same installation epoch, and the first account of a new install fixes that epoch and records it in the `.epoch` marker. Account deletion must remove the feed row (AC-27).

## Outcome

- `FeedHorizon::raise_purge_horizon(tx, account, seq)` updates `purge_horizon` in the caller's transaction; zero matching rows is not an error; a lower `seq` follows the rule decided in E-12/c01 (G-05-3).
- `AccountLifecycleHook::initialize(tx, account)` inserts the feed row with the installation epoch: `max(epoch)` of existing rows, or the clock's unix seconds on an empty install, in which case it also writes the marker through `EpochMarker::write`.
- `erase_batch(account, limit)` deletes the row and returns `remaining = 0`, also on repeat.

## Scope

### Included
- The horizon update, the hook's two methods, the first-epoch marker write.

### Excluded
- Selecting tombstones to purge and calling the horizon last (02, E-13/c10); the epoch bump (E-12/c07); triggering hooks (01 and 07).

## Acceptance Criteria

**AC-1: Raised horizon forces resync**
Given an account with seq 50 and horizon 0
When `raise_purge_horizon(tx, account, 30)` commits, the call is made for an erased account, and a caller transaction that raised it rolls back
Then cursors below 30 get `ResyncRequired{below_purge_horizon}`, the call for the erased account returns `Ok`, and the rollback leaves the horizon unchanged

**AC-2: Lower seq follows the decided rule**
Given a horizon of 30
When `raise_purge_horizon` is called with 20
Then the horizon follows the rule recorded by E-12/c01 (if the rule is "never decreases", it stays 30)

**AC-3: Installation epoch at account creation**
Given an empty install with the clock at T
When a first account is initialized and committed, and then a second one
Then the first row has epoch = unix seconds of T and the marker reads that value, and the second row has the same epoch and the marker is unchanged

**AC-4: Erase removes the row and ends holds**
Given an account with a feed row and a parked read whose revalidator reports the account gone after erase
When `erase_batch` runs twice
Then both calls return `remaining = 0`, the row is gone, and the parked read ends with `Unauthenticated`

## Non-Functional Requirements

**Reliability**
- Both writes run inside the caller's transaction; nothing is held across the marker write beyond it.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Epoch choice: empty install versus existing rows | Clock seconds; `max(epoch)` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Seq 50, horizon 0 | IT-05-12 raise, zero rows, rollback | Resync below 30; `Ok`; unchanged | AC-41, NFT-RES-08 |
| AC-2 | Horizon 30 | IT-05-12 lower-seq case (unblocked by E-12/c01) | Per rule | |
| AC-3 | Empty install, real `EpochMarker` on a temp directory | IT-05-13 initialize | Clock epoch and marker; same epoch for the second | |
| AC-4 | Feed row, parked read | IT-05-13 erase | `remaining = 0` twice; hold ends | AC-27, FT-P-19, NFT-RES-30 |

## Constraints

- `EpochMarker` is used through its port only; 05 never touches the blob volume itself.
- 12 registers this hook late, after the tree's, so the tree's erase batches can still stamp the feed.

## Risks & Mitigation

**Risk 1: Horizon raised outside the purge transaction**
- *Risk*: a horizon above the purged range hides live changes.
- *Mitigation*: the contract makes the horizon the last statement of the purge transaction (02 obligation).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 04 says the marker is written "after commit" when the installation epoch is chosen, but `initialize` runs inside the caller's transaction; if the first account's creation rolls back, the marker holds an epoch no row has until the next first-account creation or `serve` start rewrites it | Epic owner | open | Low |
| The tests of this task use the real 04 `EpochMarker`; the E-00 crate-graph test forbids edges between component crates, and it is not stated whether dev-dependencies count | E-00 | open | Medium |

## Contract

This task produces/implements the `FeedHorizon` part of the contract at `_docs/02_document/contracts/05_sync_feed/feed-writer-ports.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
