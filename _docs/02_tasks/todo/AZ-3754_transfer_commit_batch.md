# CommitService commit_batch with receipts and replay

**Task**: AZ-3754_transfer_commit_batch
**Name**: Transfer commit_batch
**Description**: Commit up to 500 staged files in one `publish` call with a client commit id: `ReceiptEffect` replays a stored response under the account lock, rejects a reused id with a different request, and stores every entry outcome
**Complexity**: 5 points
**Dependencies**: AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3711_tree_quota_gc_deltas, AZ-3766_sc_ids, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger
**Component**: 03_transfer
**Tracker**: AZ-3754
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/commit_service.md` — this task adds `commit_batch` and the receipt rules.
- `_docs/02_document/contracts/02_tree/tree_commit.md` — `on_begin` replay and `on_batch` outcomes.

## Problem

Small files reach AC-40 only through a batch commit, and a batch whose response is lost must be safe to resend: it must apply once, return the same answer, and never rely on a unique violation inside a PostgreSQL transaction. A reused commit id with different content is a client bug and must be refused. Entries fail on their own; the client re-sends failed ones under a new id (FT-N-38).

## Outcome

- `commit_batch(BatchCommit{commit_id, entries <= batch_max_entries})` returns `BatchResult{per_entry}` or `BatchTooLarge`, `CommitIdReused`, `AccountNotActive`.
- An identical resend returns the stored response with nothing applied; two identical batches at once apply exactly once.
- The receipt stores all outcomes, failed ones included, and is per account.

## Scope

### Included
- Per entry: the staged row by `staged_key`, `Target::Existing` or `New` as in E-14/c08, `NewRevision` with the row's `reserved_bytes`, `client_mtime`, `client_meta`, `import_hint`.
- `request_hash` over the batch request; `ReceiptEffect.on_begin` looks up `commit_receipts(account, commit_id)` under the account lock held by `publish`: same hash gives `Replay(stored response)`, different hash gives `EffectError::Batch(CommitIdReused)`.
- `ReceiptEffect.on_batch` inserts `commit_receipts(account, commit_id, request_hash, response)` with every outcome.
- `BlobCommitEffect` per entry (from E-14/c08); one `publish` call; metrics `commits_total{kind = batch}`.

### Excluded
- `ReceiptJanitor` (E-14/c11); client pacing and persistence of `commit_id` (E-23, E-26).

## Acceptance Criteria

**AC-1: Batch with per-entry failures (AC-40)**
Given 500 staged files and `commit_id = C1`, where entry 7 has a bad name and entry 300 a tombstoned parent
When the batch is committed
Then 498 entries are `Committed`, 2 carry typed entry errors, all share one seq, and the receipt row stores all 500 outcomes

**AC-2: Identical resend replays**
Given the committed batch C1
When the identical request is sent again
Then the stored response is returned, 0 new nodes or revisions exist, and usage is unchanged

**AC-3: Reused id and size limit**
Given C1 stored
When C1 is sent with one entry changed, and a batch of 501 entries is sent
Then the first returns `CommitIdReused` and the second `BatchTooLarge`, both with nothing applied

**AC-4: Concurrent identical batches apply once**
Given two connections
When both send the identical new batch at the same moment
Then exactly one applies, the other gets the stored response, and no unique-violation error occurs anywhere

**AC-5: Failed entries re-sent under a new id**
Given the 2 failed entries of AC-1, fixed
When they are sent in a new batch with a new `commit_id`
Then both commit once

**AC-6: Receipts are per account (AC-59)**
Given account B using commit id C1
When B commits its own batch with C1
Then B's batch applies independently and B never receives A's stored response

**AC-7: Retried folder of small files (acceptance)**
Given 500 staged files from DS-01 and a persisted `commit_id`
When the batch is committed and the identical batch is resent after a lost response
Then all 500 are committed once and the resend returns the stored response with no duplicates

## Non-Functional Requirements

**Performance**
- Batch p95 at most 1.0 s (fail 2 s) and 10,000 small files in at most 25 s server time (fail 45 s); measured by PT-03-3 in E-14/c14.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Request hash stability | Same request same hash; any changed entry differs |
| AC-4 | Receipt race | Lookup and insert under one lock; one winner |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 500 staged, 2 bad | Commit C1 (IT-03-8) | 498 + 2; one seq; receipt of 500 | FT-P-04 |
| AC-2 | C1 stored | Identical resend (IT-03-8) | Stored response; nothing new | FT-N-38 |
| AC-3 | C1 stored | Changed entry; 501 entries (IT-03-8) | `CommitIdReused`; `BatchTooLarge` | |
| AC-4 | Two connections | Concurrent identical (IT-03-8) | One applies | NFT-RES-22 |
| AC-5 | 2 failed entries | New id (IT-03-8) | Commit once | |
| AC-6 | Account B, id C1 | Commit (IT-03-8) | Independent | ST-03-1 |
| AC-7 | DS-01 subset | Commit, resend (AT-03-2) | 500 once; stored response | AC-40 |

## Constraints

- Real 02 `publish`, 04 registry and 07a on one PostgreSQL. The HTTP-level replay is pipeline AZ-3656.

## Risks & Mitigation

**Risk 1: Receipt checked outside the lock**
- *Risk*: two identical batches both pass the lookup.
- *Mitigation*: the lookup runs in `on_begin` after `publish` took the account lock; AC-4.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Pipeline AZ-3656 AC-4 oversized batch: 422 `batch_too_large` for more entries than `batch_max_entries`; a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer (resolved 2026-11-07, G-11-1) | E-21, E-14 owner | resolved | Low |
| How `request_hash` canonicalises the request (entry order, optional fields) is not stated | E-14 owner | open | Low |
