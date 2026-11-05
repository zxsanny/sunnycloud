# Blob ownership check in publish

**Task**: AZ-3712_tree_blob_ownership_check
**Name**: Tree publish blob ownership check
**Description**: Before writing a revision, `publish` checks through `BlobCatalog` that `NewRevision.blob_id` belongs to the committing account; a foreign or unknown blob gives one existing typed entry error
**Complexity**: 2 points
**Dependencies**: AZ-3709_tree_publish_core, AZ-3729_blob_registry_gc_queue
**Component**: 02_tree
**Tracker**: AZ-3712
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — commit contract; this task fixes the entry error for a foreign blob.

## Problem

`publish` receives a `blob_id` inside each `NewRevision`. If it trusts that id, account B could attach account A's blob to a node of its own and read A's bytes (ST-02-4). The Step 5 gap pass made 02 `publish` the owner of this check, through 04's `BlobCatalog`/`BlobRef`, using the existing error names `BlocklistMismatch` or `NotFound`.

## Outcome

- A revision is written only for a blob that belongs to the committing account.
- A foreign blob and a random blob id give the same error value, so existence is not revealed (AC-59).
- The failure is per entry: the rest of a batch commits.

## Scope

### Included
- The account-scoped blob lookup inside each entry savepoint, before the revision insert.
- Mapping a foreign or unknown blob to one typed entry error (`BlocklistMismatch` or `NotFound`, the same for both cases).

### Excluded
- Blob state flips and session rows (03's `BlobCommitEffect`, E-14/c08).
- The full IDOR matrix (E-13/c15).

## Acceptance Criteria

**AC-1: Foreign blob refused (AC-59)**
Given account A's committed blob and account B signed in
When B publishes a `New` entry naming A's `blob_id`
Then the entry fails with the typed error, no node or revision is written, A's blob row is unchanged, and B has no node from which A's bytes can be read

**AC-2: Same answer as a random id**
Given the AC-1 setup
When B publishes one entry with A's `blob_id` and one with a random id
Then both entries fail with the same error value

**AC-3: Per-entry failure in a batch**
Given a 3-entry batch where the middle entry names a foreign blob
When it is published
Then 2 entries are `Published`, the middle one carries the typed error, and one seq is consumed

**AC-4: Own blob accepted**
Given B's own blob in state `uploading`
When B publishes it
Then the entry is `Created`

## Non-Functional Requirements

**Performance**
- One indexed lookup per entry; within the 500-entry batch budget of PT-02-4.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Error mapping for foreign and unknown | One value for both |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A's blob; B signed in | B publishes A's blob (ST-02-4) | Typed error; nothing written | NFT-SEC-13 |
| AC-2 | Foreign and random id | Compare errors (ST-02-4) | Identical | AC-59 |
| AC-3 | 3-entry batch | Publish | 2 published, 1 failed, one seq | |
| AC-4 | B's uploading blob | Publish | `Created` | |

## Constraints

- Before E-14 lands, the test uses a recording effect; the same case with the real `BlobCommitEffect` runs in E-13/c16.

## Risks & Mitigation

**Risk 1: Check skipped on the batch path**
- *Risk*: single and batch commits use different code paths.
- *Mitigation*: AC-3 runs the check inside a batch.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Which of `BlocklistMismatch` and `NotFound` is the single answer is not fixed (02 risks (c)); E-21 maps it to the wire | E-13 owner, E-21 | open | Low |
| ST-02-4 in tests.md is still marked BLOCKED; the owner was fixed at the Step 5 gap pass | tests.md writers | resolved | Low |
