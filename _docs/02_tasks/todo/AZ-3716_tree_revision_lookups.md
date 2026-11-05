# Revision lookups, path resolution and live usage

**Task**: AZ-3716_tree_revision_lookups
**Name**: Tree revision lookups and usage source
**Description**: Serve `get_revision`, `is_current_or_retained`, the batched `lookup_revisions`, `describe_blobs`, `find_revisions` by device, `resolve_path` and `LiveUsageSource::live_bytes`
**Complexity**: 3 points
**Dependencies**: AZ-3707_tree_reader, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core
**Component**: 02_tree
**Tracker**: AZ-3716
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_reader.md` — this task adds the revision reads and `LiveUsageSource`.
- `_docs/02_document/contracts/02_tree/tree_service.md` — this task adds `resolve_path` and `find_revisions`.

## Problem

Several callers need revision data without touching the tables: an Android reinstall rebuilds its index by hash and device (AC-46), a first desktop sync checks 1,000 hashes at once (AC-31), 08 checks that a job's revision is still current or retained, `doctor` lists the paths of revisions whose blob is missing (AC-73), the CLI restores by path, and the quota drift check needs the sum of live current revisions (AC-74).

## Outcome

- Each lookup is account-scoped, bounded and paged where it can grow.
- `describe_blobs` serves `doctor` across accounts in system scope (component part of R6/AC-73).
- `live_bytes(account)` returns the sum of current revisions of live files.

## Scope

### Included
- `TreeReader::get_revision(account, revision)` with the blocklist; `is_current_or_retained(account, revision)`.
- `TreeReader::lookup_revisions(account, hashes)`: at most 1,000 hashes (`BatchTooLarge` above), current revisions of live files only, with `client_meta`.
- `TreeReader::describe_blobs(blob_ids)`: paths of the nodes using each blob, system scope.
- `TreeService::find_revisions(account, client_device, paging)`: keyset paging, `InvalidCursor`.
- `TreeService::resolve_path(account, path)`: node id or `NotFound`.
- `LiveUsageSource::live_bytes(account)`.

### Excluded
- The `doctor` aggregation (E-22/c14) and the `recompute` call (E-11/c04).

## Acceptance Criteria

AC-1 to AC-5 are local to this task. AC-73 keeps its number from `acceptance_criteria.md`.

**AC-1: Batched hash lookup (AC-31, AC-46)**
Given 1,500 files with distinct hashes, some deleted, some with replaced revisions, 200 of them with `client_meta.device`
When `lookup_revisions` is called with 1,000 hashes and with 1,001
Then the first returns only current revisions of live files with their `client_meta`, without replaced or deleted hashes, and the second returns `BatchTooLarge`

**AC-2: Device filter with keyset paging (AC-46)**
Given the same data
When `find_revisions(device)` is paged to the end, and called with a garbage cursor
Then the pages hold the 200 revisions without duplicates, and the garbage cursor returns `InvalidCursor`

**AC-3: Revision reads**
Given current, retained, collected and foreign revisions
When `get_revision` and `is_current_or_retained` are called
Then `get_revision` returns the blocklist for an own revision, and `is_current_or_retained` is true for current and retained, false for collected and foreign

**AC-73: Paths for doctor (R6, component part)**
Given 5 blobs of live files in nested folders, 1 blob of a tombstoned file and 1 random id
When `describe_blobs` is called for them, and for 1,000 ids
Then the live blobs return their full paths, the random id returns nothing, and 1,000 ids answer within 200 ms

**AC-4: Path resolution (AC-37)**
Given `R/a/b.txt`
When `resolve_path` is called with `a/b.txt`, a missing path, `../x` and `a//b`
Then the first returns the node id and the others return `NotFound`

**AC-5: Live usage source (AC-74)**
Given files that were created, replaced and deleted
When `live_bytes` is called
Then it equals the sum of the sizes of the current revisions of live files

## Non-Functional Requirements

**Performance**
- `describe_blobs` for 1,000 ids under 200 ms; `lookup_revisions` uses `(account_id, content_hash)`; the device filter uses `revisions_client_device_idx`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Path splitting via helper 06 | Empty and dot segments rejected |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 1,500 files | 1,000 and 1,001 hashes (IT-02-18) | Current live only; `BatchTooLarge` | FT-P-54, FT-P-32 |
| AC-2 | 200 with device | Paging; garbage cursor (IT-02-18) | No duplicates; `InvalidCursor` | FT-P-32 |
| AC-3 | Four revision states | Reads (IT-02-18) | Blocklist; true/false as listed | |
| AC-73 | 7 ids; 1,000 ids | `describe_blobs` (IT-02-17) | Paths; empty; < 200 ms | NFT-RES-37 |
| AC-4 | `R/a/b.txt` | Four paths (ST-02-3 part) | Id; `NotFound` x3 | NFT-SEC-22 |
| AC-5 | Mixed history | `live_bytes` | Equals live sum | IT-02-15 |

## Constraints

- The cross-component AC-73 run after a database restore belongs to the pipeline AZ-3666.

## Risks & Mitigation

**Risk 1: Large hash lists**
- *Risk*: a 1,000-hash call scans broadly.
- *Mitigation*: indexed lookup; limit enforced before the query.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-02-17 includes a tombstoned file's blob in `describe_blobs` but states no expected result (path, empty, or marked deleted) | E-13 owner | open | Low |
| Whether `resolve_path` resolves a path whose last node is tombstoned (needed by restore of a deleted file, E-13/c11) is not stated | E-13 owner | open | Medium |
