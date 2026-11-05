# TreeReader: nodes, listing pages and live-file resolution

**Task**: AZ-3707_tree_reader
**Name**: TreeReader node and listing reads
**Description**: Serve `get_node`, keyset-paged `list_children`, `find_live_node`, `resolve_live_file` with the parent-chain walk, and `root_node`, scoped by account
**Complexity**: 5 points
**Dependencies**: AZ-3706_tree_skeleton, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 02_tree
**Tracker**: AZ-3707
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/02_tree/tree_reader.md` — produced by this task (E-13/c12 and E-13/c14 extend it).

## Problem

Seven components (03, 06, 07, 08, 11, 12, 20) read the tree only through `TreeReader`. They need listings of 10,000 entries within AC-6, a clear difference between "is a folder" and "gone", and a deleted file that is gone at once, before the 30 s background cascade has marked it (AC-12, AC-77). A share link and a download must also follow a file across rename and move because its id is stable.

## Outcome

- Listings page by `name_key` with a default of 1,000 items and use only an index scan plus primary-key lookups.
- `resolve_live_file` tells file, folder and gone apart, and a file under a tombstoned ancestor is gone on the next call.
- Every read carries the account; another account's id looks exactly like a random id.

## Scope

### Included
- `get_node(account, id or Root)` returning `NodeView`; a node under a tombstoned ancestor is reported deleted (bounded parent-chain walk).
- `list_children(account, folder, after_name_key?, limit)` returning `ListingPage{items, next}`: default `listing_default_limit`, clamp to `listing_max_limit`, items with size, mtime, content hash, media type and `thumb_state` (`none` until E-13/c14 wires the source); errors `NotFound`, `NotAFolder`, `InvalidCursor`.
- `find_live_node(account, parent, name)` by `name_key`, live nodes only.
- `resolve_live_file(account, node)` returning `LiveFile{kind, revision, blob_ref}` or `Gone`.
- `root_node(account)`.
- No caching of any result.

### Excluded
- `get_revision`, `is_current_or_retained`, `describe_blobs`, `lookup_revisions` (E-13/c12).
- Thumbnail state lookup (E-13/c14); the full IDOR matrix and listing performance run (E-13/c15).

## Acceptance Criteria

**AC-1: Complete ordered paging (AC-6)**
Given the DS-02 folder of 10,000 entries (9,500 files, 500 folders, mixed case and Unicode)
When it is listed with the default limit and each `next` cursor
Then there are 10 pages, the concatenated names are 10,000 unique entries sorted by `name_key`, and the last page has no `next`

**AC-2: Limits and listing errors**
Given the same folder
When it is listed with a limit above `listing_max_limit`, with a garbage cursor, with a file's id as the folder, and with a missing id
Then the limit is clamped, and the calls return `InvalidCursor`, `NotAFolder` and `NotFound`

**AC-3: Index-only listing plan**
Given DS-02 and the runtime role `sunnycloud` with RLS on
When the page query is explained
Then the plan is an index scan on `nodes_parent_name_key` plus primary-key lookups, with no sequential scan

**AC-4: File, folder and gone are distinct**
Given a live file, a live folder, a tombstoned file, a purged file and another account's file
When `resolve_live_file` is called for each
Then it returns `LiveFile{kind = File}` with the current revision and blob ref, `LiveFile{kind = Folder}` with empty revision and blob ref, and `Gone` for the last three

**AC-5: Gone at once under a tombstoned ancestor (AC-77, AC-12)**
Given a live file inside folder A
When A is deleted and no cascade batch has run yet
Then the next `resolve_live_file` returns `Gone` and `get_node` reports the file deleted

**AC-6: Stable id across rename and move (AC-77, AC-34)**
Given a live file resolved by its node id
When the file is renamed, then moved to another folder, then deleted
Then the same id still resolves after the rename and after the move, and returns `Gone` on the first call after the delete

**AC-7: Live lookup by folded name**
Given `A.txt` live and `b.txt` tombstoned in one folder
When `find_live_node` is called for `a.txt` and for `b.txt`
Then the first returns the `A.txt` node and the second returns nothing

**AC-8: Foreign ids look like random ids (AC-59)**
Given accounts A and B
When B calls `get_node`, `list_children`, `find_live_node` and `resolve_live_file` with A's node and folder ids, and `root_node` for itself
Then B gets `NotFound`, nothing or `Gone`, equal to the result for random ids, never A's data, and `root_node` returns B's own root

## Non-Functional Requirements

**Performance**
- One 1000-entry page p95 at most 40 ms (fail 80 ms); full 10,000-entry listing p95 at most 200 ms (fail 300 ms); measured by PT-02-1 in E-13/c15.
- The parent-chain walk is bounded by tree depth (about 30).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Cursor decoding and limit clamping | Garbage cursor rejected; limit clamped |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-02 template database | Default paging (IT-02-16) | 10 pages, 10,000 unique sorted names | AC-6, FT-P-09 |
| AC-2 | DS-02 | Clamp, garbage cursor, file id, missing id (IT-02-16) | Clamp; `InvalidCursor`; `NotAFolder`; `NotFound` | |
| AC-3 | DS-02, role `sunnycloud` | EXPLAIN of the page query (IT-02-16) | Index scan plus PK lookups only | NFT-PERF-10 |
| AC-4 | Five node states (IT-02-17) | `resolve_live_file` | File, Folder, Gone x3 | AC-77 |
| AC-5 | File under A, A deleted, no cascade | `resolve_live_file`, `get_node` (IT-02-17 variant) | Gone at once | FT-N-44 |
| AC-6 | Resolved file | Rename, move, delete, resolve (IT-02-17) | Same id resolves; Gone after delete | FT-N-44 |
| AC-7 | `A.txt` live, `b.txt` tombstoned | `find_live_node` | Folded match; tombstone ignored | |
| AC-8 | Accounts A and B | Foreign ids (ST-02-1 subset) | Same as random ids | NFT-SEC-13 |

## Constraints

- Test data: DS-02 is loaded once through batch `publish` into the template database `tmpl_listing` (02 tests.md), which is why this task follows E-13/c04 and E-13/c05.
- Real PostgreSQL 18 with the runtime role; no in-memory store.

## Risks & Mitigation

**Risk 1: Parent-chain walk on every lookup**
- *Risk*: the walk adds latency to hot reads.
- *Mitigation*: bounded depth; measured in PT-02-1 and NFT-PERF-10.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `listing_max_limit` default is not stated (02 §6) | E-13 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/02_tree/tree_reader.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
