# FeedLoop: one long-poll and one cursor per account, fan-out by ancestry

**Task**: AZ-3904_feed_loop
**Name**: Feed loop
**Description**: One feed loop per account long-polls the change feed through sync-core, hands each row to every pair it touches by ancestry, saves the single cursor only after the rows up to it were applied, re-reads deferred rows, and on `ResyncRequired` reads the head cursor and runs the first-sync merge before moving on
**Complexity**: 3 points
**Dependencies**: AZ-3891_local_index_store, AZ-3905_change_applier, AZ-3906_first_sync_merger, AZ-3965_feed_client, AZ-3763_sc_config
**Component**: 14 desktop_daemon
**Tracker**: AZ-3904
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c12a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `poll_changes`, `head_cursor`, `ResyncRequired` (E-23/c12 contribution).

## Problem

All pairs of one account share one server feed. Polling per pair would multiply long-polls and cursors and could apply one change twice. A cursor saved before its rows are applied loses changes after a crash; one saved too late re-applies them, which must then be harmless (AC-69). After a server restore or a 91-day absence the feed answers `ResyncRequired`, and the daemon must re-read the tree without deleting anything (AC-41).

This is the first half of the split E-24/c12 row: the planned row (feed loop, change applier, dirty-path rule, trash) is over 5 points; the change applier with the dirty-path rule and trash is E-24/c12b.

## Outcome

- At most one `poll_changes` in flight per account, with `wait` from `desktop.longpoll_wait` (30 s, capped by the server) and the cursor from `meta`.
- Each row goes to every pair it belongs to: its `node_id` is an entry of the pair, or its `parent_id` is an entry or the root node of the pair; a node moved between pairs is a delete in one and a download in the other.
- The cursor is saved with `set_cursor` only after every row before it was applied or settled by its pair; a row the applier defers (dirty-path rule) holds the cursor back until its upload result is stored, then is re-read from the cursor and applied.
- `ResyncRequired` (410, or `invalid_cursor` mapped by 13) runs `head_cursor`, then the first-sync merge (E-24/c13) for every pair, and only then stores the new cursor.
- `Unauthorized` ends the loop within 5 s and signals signed-out to the login state (E-24/c15); transport errors back off and retry.
- The `desktop.longpoll_wait` row added to helper 01's key table.

## Scope

### Included
- The per-account loop, fan-out, cursor rule, deferred-row re-read, resync order, error handling.

### Excluded
- Applying one row to one pair (E-24/c12b); the merge itself (E-24/c13); long-poll mechanics and page invariants (E-23/c12); starting the loop in the running daemon (E-24/p03).

## Acceptance Criteria

**AC-1: One loop and one cursor per account**
Given an account with pairs P1 and P2
When the daemon runs
Then at most 1 `poll_changes` is in flight and `meta` holds 1 cursor row for the account

**AC-2: Fan-out by ancestry**
Given 500 feed rows of which 40 touch both pairs, including nodes moved from P1 to P2
When they are applied
Then each row reaches exactly the pairs it belongs to, and a node moved between pairs is a delete in P1 and a download in P2

**AC-3: Kill between apply and cursor save (AC-69)**
Given the loop killed after page 3 was applied and before `set_cursor`
When it restarts
Then page 3 is applied again with no duplicate file, no extra upload and identical final trees

**AC-4: Deferred row holds the cursor**
Given a row for a dirty path that the applier defers
When the local upload result is stored
Then the cursor never passed that row before, and the row is re-read from the cursor and applied after the result

**AC-5: Resync order (AC-41)**
Given a fake `SyncApi` answering `ResyncRequired` (410, and 422 `invalid_cursor` mapped by 13)
When the loop receives it
Then the calls are `head_cursor` first, then the merge's `list_children` pages, and the new cursor is stored only after the merge finished; no delete is sent

**AC-6: Revoked token stops the loop**
Given a poll answered `Unauthorized`
When the loop handles it
Then it ends within 5 s, signals signed-out, and keeps the pairs and the index

## Non-Functional Requirements

**Performance**
- Feed rows reach the applier without waiting for the next poll; AC-5 (5 s at p95 for a change up to 10 MB) is measured end to end in E-31.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Two pairs, fake `SyncApi` counting in-flight polls | <= 1 in flight; one cursor row |
| AC-2 | 500 rows, 40 on both pairs, moves between pairs | Exact pair sets; delete plus download |
| AC-4 | Deferred row, then upload result | Cursor held; row re-applied after |
| AC-5 | 410 and 422 variants | `head_cursor`, then listing; cursor after merge; 0 deletes |
| AC-6 | `Unauthorized` from `poll_changes` | Loop ends <= 5 s; signed-out signalled; index kept |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | Server; two pairs; kill after page 3 | IT-14-6 | One poll in flight; one cursor; idempotent re-apply | AC-69, NFT-RES-35 |
| AC-5 | DB restored from an older backup (`e2e-ctl`); device offline 91 days (test clock) | IT-14-13 | `head_cursor` then listing; converge within 60 s; deletions only via the guard | AC-41, FT-P-13, FT-P-14, NFT-RES-07 |
| AC-6 | Device revoked via the API while the loop is held | IT-14-21 (loop half) | Loop ends within 5 s | AC-21 |

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- Test-clock cases are group `X`.

## Risks & Mitigation

**Risk 1: A deferred row blocks the feed for long**
- *Risk*: a large dirty upload delays every later row.
- *Mitigation*: rows of other pairs and other paths still apply; only the cursor waits.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-24/c12 was split into c12a and c12b; E-24/p03 depends on "E-24/c12" and must resolve to both | Parent (Step 4) | open | Low |
