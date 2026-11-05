# ChangeFeed read and head: projection, seq-boundary pages, cursor rules

**Task**: AZ-3776_change_feed_read
**Name**: Change feed read and head
**Description**: Read the latest state of every node changed after a cursor in pages that end on a `seq` boundary, judge every stale, foreign-epoch, future or malformed cursor, and return the head cursor
**Complexity**: 5 points
**Dependencies**: AZ-3774_sync_feed_skeleton, AZ-3775_feed_stamp
**Component**: 05 sync_feed
**Tracker**: AZ-3776
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c04

## Problem

A device catches up after a suspend by reading from its cursor (AC-35); after a restore or a long absence it must be told to resync instead of silently missing changes (AC-41). The feed is not an event log: it returns the latest state of each changed node, so a page must never split nodes that share one `seq` or a device would see half a batch.

## Outcome

- Cursor text `E.S` parses to `Cursor{epoch, seq}`; anything else is `BadCursor` (wire `invalid_cursor`).
- A cursor is judged against the account's feed row: `E != epoch` -> `ResyncRequired{epoch_changed}`, `S < purge_horizon` -> `below_purge_horizon` (`S == purge_horizon` is valid), `S > seq` -> `cursor_ahead`.
- A valid cursor returns `ChangePage{items, next_cursor, has_more}`: `ChangeItem{node_id, parent_id, kind, name, state, revision_id?, size?, content_hash?, seq}` for nodes with `seq > S`, ordered by `seq` then id, ending on a `seq` boundary even if that exceeds `limit` (default 500, capped by `feed_page_limit_max`).
- `head(account)` returns the current `epoch.seq`.
- Reads are scoped by the caller's account and use `nodes_feed_idx`.

## Scope

### Included
- Cursor parsing and classification, the page query and projection, paging rules, `head`.

### Excluded
- Holding an empty read (wait > 0), revalidation and shutdown (E-12/c05). The `ChangeFeed` port is complete only after E-12/c05; this task delivers the immediate read path and `head`, with no placeholder method.
- HTTP routes and status mapping (E-21/c13).

## Acceptance Criteria

**AC-1: Latest state per node**
Given node A stamped at seq 1 and again at seq 5 after a rename, and folder F tombstoned at seq 6
When the feed is read from `E.0`
Then A appears once with seq 5 and its new name, F appears with state tombstone, and every item carries all `ChangeItem` fields

**AC-2: Pages end on a seq boundary**
Given 500 nodes sharing seq 7 and `limit = 100`
When the page containing seq 7 is read
Then it holds all 500 items, `has_more` is false after it, `next_cursor` is `E.7`, and a read from `E.7` returns an empty page

**AC-3: Resume and revival**
Given the data of AC-1 and AC-2
When the feed is read from `E.5`, and a folder that was a tombstone is stamped live again
Then only seq 6 and 7 are returned, and the revived folder appears with state live at its new seq

**AC-4: Cursor rules**
Given a feed with epoch E, seq 50 and purge horizon 20
When cursors `E+1.10`, `E.19`, `E.20`, `E.50`, `E.51` and `E.9999999999` are read
Then they give `ResyncRequired{epoch_changed}`, `ResyncRequired{below_purge_horizon}`, a normal page, an empty page, `ResyncRequired{cursor_ahead}` and `ResyncRequired{cursor_ahead}`

**AC-5: Malformed cursors**
Given the strings `""`, `abc`, `1.`, `.5`, `1.2.3`, `-1.5` and `99999999999999999999.1`
When each is used as a cursor
Then each returns `BadCursor` and none causes a database error

**AC-6: Head**
Given an account after k stamps, and a second account
When `head` is read, a new device lists the tree after taking head, and then reads from that head
Then head is `E.k`, the read returns only later changes, and the other account's head is independent

**AC-7: Reader sees concurrent stamps once (IT-05-2 reader part)**
Given the 50-writer workload of E-12/c03 running
When a reader loops from `E.0` until the end
Then `seq` never decreases between pages, every committed node appears once with its latest state, and no committed node is missing at the end

**AC-8: Page cost (PT-05-2)**
Given an account with 1,000,000 nodes, 500-item pages and 20 concurrent readers doing 5,000 reads, plus a 500-item catch-up after a 30-minute gap, on REF-SRV
When the run finishes
Then page latency p95 is at most 20 ms (failure 60 ms) and the query plan is an index scan on `nodes_feed_idx` with no sequential scan

## Non-Functional Requirements

**Performance**
- As AC-8; at most 8 connections for the readers.

**Compatibility**
- Clients ignore unknown item fields; new fields are additive.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Classification table | Each cursor classified as listed |
| AC-5 | Cursor parser over the cursor corpus | `BadCursor` for every malformed string |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stamped fixtures | IT-05-3 projection | Latest state once; all fields | AC-8 |
| AC-2 | 500 nodes at seq 7 | IT-05-3 boundary | One page of 500; empty after | |
| AC-3 | Cursor `E.5`; revived folder | IT-05-3 resume and revival | seq 6 and 7; live at new seq | AC-35, FT-P-58 |
| AC-4 | Epoch E, seq 50, horizon 20 | IT-05-4 | Classified as listed | AC-41, FT-P-13, FT-P-14 |
| AC-5 | Cursor corpus | IT-05-4 malformed strings | `BadCursor`, no SQL error | |
| AC-6 | k stamps; second account | IT-05-5 | `E.k`; independent heads | |
| AC-7 | E-12/c03 workload | IT-05-2 reader part | Non-decreasing seq; each node once | NFT-RES-35 |
| AC-8 | `tmpl_1m` template, 20 readers | PT-05-2 | p95 <= 20 ms; index scan | NFT-PERF-08 |

## Constraints

- Every query filters by the caller's `account_id`; the counter row is never exposed.
- PT-05-2 timing on REF-SRV only; elsewhere `NOT-RUN(HW-REF)`.

## Risks & Mitigation

**Risk 1: A large batch makes one big page**
- *Risk*: 500 items sharing one `seq` exceed `limit`.
- *Mitigation*: documented in the contract (a page may exceed `limit`); 500 is the batch cap of 02.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Which reason wins when a cursor breaks more than one rule (for example another epoch and `S > seq`) is not stated | Epic owner | open | Low |
| `feed_page_limit_max` has no value in the specs (helper config table lists the key only) | Epic owner / E-01 | open | Low |

## Contract

This task produces/implements the read, head, item and cursor parts of the contract at `_docs/02_document/contracts/05_sync_feed/change-feed.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
