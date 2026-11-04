# Test Specification — 02_tree
Tier: component integration (`cargo test -p tree`) at the public port boundary: `TreeService`, `TreeReader`, `TreeCommit`, `TreeRestore`, `LiveUsageSource`, `TreeMaintenance`/`Janitor`, `AccountLifecycleHook`, `BlobOwnerCleanup`. Env class `C`; timing rows `C-REF`. System scenarios are referenced by ID, not restated.

Collaborators are the real implementations behind their ports, on one real PostgreSQL 18 (ADR 015, no in-memory stores): 01 `AccountGuard`, 05 `FeedStamp`/`FeedNotifier`/`FeedHorizon`/`ChangeFeed`, 07a `QuotaLedger`/`QuotaAdmin`, 04 `GcQueue`/`BlobRegistry` (blob volume is a temp directory on the runner). `CommitEffect` and `ThumbnailStateSource` are supplied by the tests: a recording effect, a failing effect (Entry and Batch), a replay effect and a fixed thumbnail-state map (these are per-call ports, not stores). The clock is the manual test clock.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|------------------------------|----------|----------|
| AC-6 | Listing 10,000 entries p95 <= 300 ms, paged | IT-02-16, PT-02-1 | Covered here + FT-P-09, NFT-PERF-10 |
| AC-8 | Later commit wins; conflict copy; edit beats delete | IT-02-5, IT-02-6, IT-02-7, IT-02-8, AT-02-1 | Covered here + FT-P-10, FT-P-11, FT-P-12 |
| AC-27 | Account purge removes tree rows; deleting account refused | IT-02-10, IT-02-23 | Covered here (tree hook) + FT-P-19, NFT-RES-30, NFT-SEC-27 |
| AC-30 | Blob queued on delete/overwrite; operator restore | IT-02-5, IT-02-18, IT-02-21, IT-02-23 | Covered here + FT-P-16, FT-P-18, NFT-RES-09 |
| AC-31 | Hash lookup for first-sync merge | IT-02-18 | Covered here (server side) + FT-P-54 |
| AC-34 | Stable node id across save-in-place | IT-02-5, IT-02-14 | Covered here + FT-P-57 |
| AC-37 | Server name rules | IT-02-1, ST-02-3 | Covered here + FT-N-41, NFT-SEC-22 |
| AC-41 | Purge horizon raised with purge | IT-02-20 | Covered here + FT-P-13, FT-P-14, NFT-RES-07, NFT-RES-08 |
| AC-42 | Case-insensitive unique, case-preserving | IT-02-1, IT-02-3 | Covered here + FT-P-15, FT-N-04 |
| AC-46 | Device and hash lookup for reinstall | IT-02-18 | Covered here (server side) + FT-P-32 |
| AC-59 | Two-account matrix | ST-02-1, ST-02-2 | Covered here + NFT-SEC-13, NFT-SEC-14 |
| AC-68 | No committed revision references a missing blob | IT-02-12 | Covered here (tree half) + NFT-RES-34 |
| AC-69 | 50 concurrent commits, feed exactly once in order | IT-02-13, PT-02-3 | Covered here + NFT-RES-35 |
| AC-70 | Move vs edit: edit in moved folder, same id, 0 copies | IT-02-14 | Covered here + FT-N-42 |
| AC-73 | `describe_blobs` serves doctor | IT-02-17 | Covered here (port) + NFT-RES-37 |
| AC-74 | Usage = sum of live current revisions, <= quota | IT-02-4, IT-02-15 | Covered here + NFT-RES-38 |
| AC-77 | `resolve_live_file` makes link follow rename/move, delete = gone | IT-02-17 | Covered here (port half) + FT-N-44 |
| AC-5, AC-69 (latency share) | Stamp last, wake after COMMIT | IT-02-10, PT-02-3 | Covered here + NFT-PERF-08, NFT-PERF-09 |

## Blackbox Tests

### IT-02-1: Name rules and case-insensitive uniqueness
**Summary**: `NameKey` rules are enforced by `create_folder` and `update_node`; accepted and rejected names match AC-37/AC-42.
**Traces to**: AC-37, AC-42
**Description**: create folders and rename with the inputs below under root.
**Input data**:
```
reject (NameInvalid): "a/b", "a\0b", ".", "..", "x\u0007y", "x‮y", 256-byte name, ""
accept: "CON", "a:b", "..a", "trailing.", ".hidden", "emoji-\u{1F600}", 255-byte name, "A.txt"
case: create "Docs" then "docs" -> NameConflict; "é" then "é" (NFD vs NFC) -> NameConflict
```
**Expected result**:
```
Rejected names: NameInvalid(reason) and 0 rows written; accepted names stored case-preserving (list returns exact spelling)
"docs" after "Docs": NameConflict (unique index is the arbiter, no 500); NFC/NFD pair: NameConflict
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, helper 06

### IT-02-3: update_node rename and move
**Summary**: rename/move error set, id stability and advisory-lock use.
**Traces to**: AC-42, AC-70
**Description**: tree `R/A/B/f.txt`; also `create_folder` under a random id, a tombstoned folder, a file and another account's folder (-> `NotFound`, `ParentDeleted`, `NotAFolder`, `NotFound`, 0 rows). Cases: case-only rename `f.txt`->`F.TXT`; move A into B (cycle); move into own descendant 30 levels deep; rename root; move into tombstoned folder; move onto a taken name; move into a file; move a node while a second connection holds `pg_advisory_xact_lock` of the account.
**Expected result**:
```
case-only rename: same node id, new spelling; Cycle (409) for A->B and the deep case (cycle check depth ~30 completes < 100 ms);
IsRoot; ParentDeleted; NameConflict; NotAFolder; move blocks until the other connection releases the account lock, then succeeds
every success stamps once (one seq); failures stamp nothing (account_feeds.seq unchanged)
```
**Max execution time**: 8 s; **Dependencies**: PostgreSQL, real 05

### IT-02-4: delete_node and quota delta
**Summary**: delete is idempotent, honours `base_revision`, tombstones a folder with one write and cascades in batches; usage falls by the exact bytes.
**Traces to**: AC-8, AC-74
**Description**: file f (10 MiB, current r2), folder F with 100 files of 1 KiB; `cascade_batch_size = 20`.
**Input data**: `delete_node(f, base=r1)` (stale), `delete_node(f, base=r2)` twice, `delete_node(F)`, then `cascade_batch(now, 20)` until 0.
**Expected result**:
```
stale base -> EditedSince{current_revision = r2}, usage unchanged; correct base -> () and usage -10 MiB; repeat -> () with no second delta
folder delete: 1 tombstone + cascade_pending in one transaction; cascade_batch returns 20,20,20,20,20 then 0 (5 batches); usage reduced by exactly 100 KiB across batches
QuotaAdmin::view.used == LiveUsageSource::live_bytes after every batch; blobs of deleted revisions queued (GcQueue::pending_for_account grows by 101)
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, real 04 `GcQueue`, 07a

### IT-02-5: publish outcomes for new, update, replace and no-change
**Summary**: each `PublishOutcome` is produced and has the specified side effects.
**Traces to**: AC-8, AC-30, AC-34
**Description**: with a recording effect: publish `Target::New` (Created); `Existing{base = current}` (Updated, then Replaced when the old revision is non-current); same hash again (NoChange); `NewRevision.vault = true` flag carried to the effect.
**Expected result**:
```
Created: node + revision rows, quota +size, 1 stamp; Updated: same node id, new current_revision_id, old revision row kept, old blob scheduled (Replaced, Due::AfterRetention; blob_gc due_at = now + retention);
NoChange{existing_revision}: no stamp, no quota change, receipt-type effects still run; PublishedEntry received by the effect has node, revision, hash, media type, size, import_hint, vault
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, 04, 05, 07a

### IT-02-6: Stale base becomes a conflict copy
**Summary**: a stale `base_revision` publishes the new content as current and re-parents the old revision to a conflict node without copying bytes.
**Traces to**: AC-8
**Description**: node N with r1; device D1 publishes r2 (base r1); device `phone-2` publishes r3 with base r1 at clock 2026-11-04 09:07 UTC. Variants: name already taken as conflict copy; stem of 250 bytes.
**Expected result**:
```
N.current = r3; conflict node named "<stem> (conflict copy 2026-11-04 09-07 phone-2).<ext>" holds the r2 revision ROW (same revision_id and blob_id, count(revisions) increased only by r3)
outcome ConflictCopied{copy_node}; both nodes in the feed at the same seq; usage = size(r3) + size(r2)
second collision -> numeric suffix; long stem truncated so the full name <= 255 bytes
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, 05, 07a

### IT-02-7: New file whose name is taken
**Summary**: the server gives the new file the conflict-copy name; no error, no client retry.
**Traces to**: AC-8, AC-42
**Input data**: `A.txt` exists; publish `Target::New{root, "a.txt"}`; concurrently publish two more `New` entries for the same name.
**Expected result**: `CreatedAsCopy{node}` for each loser with distinct names; original untouched; no `name_conflict` code anywhere; the unique index never aborts the transaction (savepoint).
**Max execution time**: 5 s; **Dependencies**: PostgreSQL

### IT-02-8: Edit beats delete, revival of ancestors
**Summary**: publishing on a tombstoned file revives it and its tombstoned ancestors without a conflict copy.
**Traces to**: AC-8, AC-41
**Description**: `R/A/B/f` and `R/A/B/g`; delete A (tombstone + cascade run so B, f, g are marked); publish `Existing{f, base = f.current}`. Variant: name `f` re-taken meanwhile.
**Expected result**:
```
f, B, A live; cascade_pending cleared on A and B; g stays deleted; no conflict copy; ChangeFeed shows A and B tombstone->live and f revision, all at one seq
variant: revived f gets a conflict-copy name; quota +size(new revision) only
```
**Max execution time**: 6 s; **Dependencies**: PostgreSQL, 05

### IT-02-9: Savepoint per entry in a 500-entry batch
**Summary**: failed entries roll back alone; the rest commit under one `seq`.
**Traces to**: AC-8, AC-69, AC-74
**Description**: batch of 500 `New` entries; entry 10 has a tombstoned parent, 250 an invalid name, 400 triggers `EffectError::Entry(UploadNotFound)` from the test effect after `release_reservation`.
**Expected result**:
```
Published(Vec<EntryResult>) with 497 Published and 3 typed failures (ParentDeleted, NameInvalid, EffectFailed(code));
account_feeds.seq advanced by exactly 1; all 497 nodes carry the same seq; entry 400's reservation release rolled back (reserved_bytes unchanged for it), others released
transaction remained usable after each failure (no "current transaction is aborted")
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, 05, 07a

### IT-02-10: Effect protocol and statement order
**Summary**: `ensure_active` first, then the advisory lock, `on_begin`, entries, `on_batch`, stamp last, COMMIT, notify after.
**Traces to**: AC-69
**Description**: capture the statement log (pg `log_statement=all`) and effect call order for four runs: normal; `on_begin` returns `Replay`; `on_batch` returns `Batch` error; all entries `NoChange`.
**Expected result**:
```
Normal: ensure_active (FOR SHARE) < advisory lock < on_begin < per-entry savepoints < on_batch(all outcomes, failed ones included) < stamp CTE < COMMIT; 0 statements between stamp and COMMIT; notify only after COMMIT
Replay: Replayed(response), no node write, no stamp, no quota call; Batch error: whole transaction rolled back, no seq consumed
all NoChange: no stamp, no notify, effect writes (a receipt row) still committed
on_begin observed from a second connection: pg_try_advisory_xact_lock(account) == false (lock held while the effect runs)
deleting account (after AccountAdmin::begin_deletion): AccountNotActive for the whole call, 0 rows, no quota change, no seq consumed (spec gap: create_folder/update_node/delete_node do not call the guard)
```
**Max execution time**: 8 s; **Dependencies**: PostgreSQL, 01, 05

### IT-02-12: No committed revision names an uncommitted blob (tree half of AC-68)
**Summary**: a revision exists only in the transaction in which the blob effect flips the blob to `committed`; FK RESTRICT rejects dangling references.
**Traces to**: AC-68
**Description**: 300 random publishes where the effect fails (Entry/Batch) with probability 0.3, plus a publish whose `blob_id` has no `blobs` row.
**Expected result**:
```
Invariant probe after every publish: SELECT count(*) FROM revisions r LEFT JOIN blobs b ON b.id = r.blob_id WHERE b.id IS NULL OR b.state <> 'committed' AND r.id IN (committed set) -> 0
dangling blob_id -> typed failure (FK violation mapped, not a panic), no revision row
```
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, 04 `BlobRegistry::mark_committed_in_tx` in the test effect

### IT-02-13: 50 concurrent commits, feed exactly once, in order
**Summary**: concurrent `publish` calls on one account produce a gapless `seq` run readable once from cursor 0.
**Traces to**: AC-69
**Description**: round A: 50 tasks each `publish` one new file at a barrier; round B: 50 tasks each update a different file twice; a reader runs `ChangeFeed::read` from `epoch.0` with `limit = 7` concurrently and again after the writers stop.
**Expected result**:
```
Round A: seq values {1..50} each exactly once across node rows, no gap, no duplicate; account_feeds.seq == 50
Reader pages: seq non-decreasing, each page ends on a seq boundary, union covers 50 nodes, each node once (latest state); resumed reader from any intermediate cursor sees only newer items
Round B: final seq == 150; every node's final seq equals its last commit
Repeated 100 times: 0 failures
```
**Max execution time**: 120 s; **Dependencies**: PostgreSQL, real 05

### IT-02-14: Move races edit (AC-70)
**Summary**: `update_node` (move) and `publish Existing` on a file inside serialise on the account lock and the edit always lands in the moved folder.
**Traces to**: AC-70, AC-34
**Description**: folder F with file f (rev r1). Barrier-start `update_node(F -> G)` and `publish(Existing{f, base = r1})` on two connections; 500 iterations with random start skew 0-5 ms, both orders forced in 50 iterations each.
**Expected result**:
```
Every iteration: f.node_id unchanged, f.parent = F, F.parent = G, f.current = new revision, count(conflict copies) = 0, quota +size once, both changes in the feed
```
**Max execution time**: 60 s; **Dependencies**: PostgreSQL, 05, 07a

### IT-02-15: Quota stays exact under mixed commits and deletes
**Summary**: stored usage equals the sum of live current revisions and never exceeds the quota.
**Traces to**: AC-74
**Description**: quota 64 MiB; 1,000 concurrent operations on one account: 600 `reserve`+`publish` (1 KiB-1 MiB, some replace, some stale-base), 300 `delete_node`, 100 cascade batches; a sampler reads `QuotaAdmin::view` every 5 ms. Variant with `NewRevision.reserved_bytes` of a reaped session (effect fails Entry).
**Expected result**:
```
During: sampled used <= 64 MiB always; end: view.used == LiveUsageSource::live_bytes == QuotaAdmin::recompute(live_bytes, reserved_bytes) result; reserved_bytes == sum of open reservations
refused reservations (QuotaExceeded) never reach publish; failed entries leave used and reserved unchanged
```
**Max execution time**: 90 s; **Dependencies**: PostgreSQL, 07a, 05

### IT-02-16: Listing and keyset paging
**Summary**: `list_children` over the DS-02 folder is complete, ordered by `name_key`, de-duplicated and cursor-safe.
**Traces to**: AC-6
**Description**: 10,000 entries (9,500 files, 500 subfolders, mixed case/Unicode); fixed thumbnail map for 300 image items.
**Expected result**:
```
default limit 1000 -> 10 pages; concatenated names = 10,000 unique, sorted by name_key; next absent on the last page
limit > listing_max_limit clamped; garbage cursor -> InvalidCursor; folder id that is a file -> NotAFolder; missing -> NotFound
ThumbnailStateSource::states called once per page, only with hashes of image/video items; with no implementation all items thumb_state = none
EXPLAIN of the page query: index scan on nodes_parent_name_key + PK lookups, no Seq Scan, run under role sunnycloud
```
**Max execution time**: 20 s; **Dependencies**: PostgreSQL, DS-02

### IT-02-17: resolve_live_file and get_node
**Summary**: file, folder and gone are distinguishable; foreign ids are gone.
**Traces to**: AC-77, AC-59
**Input data**: live file, live folder, tombstoned file, purged file, other account's file; share-like sequence: rename, move, delete then call again; `describe_blobs` for 5 blob ids of live files in nested folders, 1 of a tombstoned file, 1 random (live -> full path, random -> empty, 1000 ids < 200 ms).
**Expected result**: `LiveFile{kind=File, revision, blob_ref}`, `{kind=Folder}` with empty revision/blob_ref, `Gone` x3; after rename/move the same node id still resolves; after `delete_node` `Gone` on the next call (no cache).
**Max execution time**: 3 s; **Dependencies**: PostgreSQL

### IT-02-18: Revision lookups
**Summary**: `lookup_revisions` and `find_revisions` return the right rows and limits.
**Traces to**: AC-31, AC-46
**Input data**: 1,500 files with distinct hashes (some deleted, some with replaced revisions), `client_meta.device` on 200; hash lists of 1000 and 1001.
**Expected result**: also `get_revision` returns the blocklist and `is_current_or_retained` is true for current and retained revisions, false for collected or foreign ones. 1000 hashes ok returning only current revisions of live files with `client_meta`; 1001 -> `BatchTooLarge`; `find_revisions(device)` pages by keyset, no duplicates, `InvalidCursor` on garbage; replaced/deleted hashes absent.
**Max execution time**: 8 s; **Dependencies**: PostgreSQL

### IT-02-20: Cascade and purge maintenance
**Summary**: janitors drain by query alone, are restart-safe, and raise the purge horizon in the same transaction.
**Traces to**: AC-41, AC-27
**Description**: `tombstone_horizon = 2 s`, `purge_batch_size = 10`; 100 tombstoned nodes; abort the first run after 2 batches (drop the connection mid-run), rerun; then read the feed with a cursor below the new horizon.
**Expected result**:
```
Re-run finishes with the remaining batches, total processed 100, no node purged twice; blobs of purged revisions queued
account_feeds.purge_horizon == highest purged seq (UPDATE is the last statement of each purge tx); ChangeFeed::read(cursor below horizon) -> ResyncRequired{below_purge_horizon}; cursor at/above -> normal page
```
**Max execution time**: 15 s; **Dependencies**: PostgreSQL, 05, 04

### IT-02-21: Operator restore re-points the revision row
**Summary**: restore never creates a second revision for a blob.
**Traces to**: AC-30
**Description**: file with r1 (t1), r2 (t2), then deleted at t3; `restore_revision(account, path, at = t1+ε)`. Variants: original folder deleted; name taken; `at` before retention; blob file absent; `reinstate` refusal injected by removing the `blobs` row.
**Expected result**:
```
count(revisions) unchanged; restored revision becomes current of the revived original node (placement Original); folder deleted -> node under root folder "Restored" (created once), revision row re-parented (UPDATE revisions SET node_id)
name taken -> conflict-copy name; previously current revision queued Replaced; blob_gc row of the restored blob cleared, state committed
BeyondRetention / FileMissing returned typed; any refusal rolls back everything (no stamp, no quota delta); success: usage +restored -replaced, one stamp
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, 04 `reinstate_in_tx`, 05, 07a

### IT-02-23: Account erase hook
**Summary**: `erase_batch` removes revision rows and queues blobs atomically, nodes leaf first, root last, and reports honest `remaining`.
**Traces to**: AC-27, AC-30
**Description**: account with 1,000 nodes and 600 revisions; `erase_batch(limit = 50)` loop; kill the process between batches once; run sweeper later.
**Expected result**:
```
After every batch: no revision row without its blob_gc row (Due::Now) and no blob_gc row for a surviving revision; no advisory lock taken
remaining = nodes + revisions + pending_for_account; non-increasing; reaches 0 only after the GcSweeper has removed all files; root deleted last; second run after restart finishes
initialize() creates the root node (one root row); BlobOwnerCleanup::remove_owner_rows deletes a current, a replaced and a tombstoned-file revision (nodes.current_revision_id cleared by the FK, RESTRICT never blocks, second call no-op)
```
**Max execution time**: 30 s; **Dependencies**: PostgreSQL, 04 `GcSweeper`

## Performance Tests

### PT-02-1: Listing 10,000 entries
**Summary**: server-side listing latency against AC-6.
**Traces to**: AC-6
**Load scenario**:
- Concurrent users: 8 clients paging the DS-02 folder (10 pages of 1000), also 1 client with `limit = 10000`
- Request rate: 20 listings/s
- Duration: 5 minutes, ramp-up 30 s

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Latency p95 (full 10,000-entry listing, summed pages) | <= 200 ms | 300 ms |
| Latency p95 (one 1000-entry page) | <= 40 ms | 80 ms |
| Error rate | 0% | 0% |
**Resource limits**: DB connections <= 8; CPU <= 2 cores. Env C-REF (system: NFT-PERF-10).

### PT-02-3: Hot account contention
**Summary**: 50 writers on one account, stamp serialisation at the tail.
**Traces to**: AC-69, AC-5
**Load scenario**: 50 concurrent writers on one account, 1,000 commits, 60 s.

| Metric | Target (derived; calibrate on REF-SRV) | Failure Threshold |
|--------|--------|-------------------|
| Throughput | >= 200 commits/s | 100 commits/s |
| Latency p95 | <= 250 ms | 600 ms |
| Seq gaps/duplicates | 0 | 0 |
**Resource limits**: DB connections <= 16. Env C-REF (system: NFT-PERF-09).

### PT-02-4: 500-entry batch and cascade batch
**Summary**: batch commit and a 5,000-node cascade batch time.
**Traces to**: AC-40, AC-74
**Load scenario**: 20 sequential 500-entry batches (AC-40 shape: 10,000 files), then `cascade_batch(5000)`.

| Metric | Target (derived from AC-40: 20 batches inside 60 s) | Failure Threshold |
|--------|--------|-------------------|
| Batch publish p95 | <= 1.0 s | 2.0 s |
| Cascade batch (5,000 nodes) | <= 2 s | 5 s |
**Resource limits**: one transaction at a time; node-lock hold time = batch duration. Env C-REF.

## Security Tests

### ST-02-1: IDOR on every TreeService/TreeReader operation
**Summary**: account B using account A's node/revision/parent ids gets nothing.
**Traces to**: AC-59
**Attack vector**: foreign ids as `node id`, `parent id`, `revision id`, `Target::Existing.node_id`, `PublishEntry.parent_id`, `describe`/`lookup` hashes.
**Test procedure**: accounts A (rich tree) and B (empty); as B call: `get_node`, `list_children`, `create_folder(parent=A.folder)`, `update_node` (both id and new parent), `delete_node`, `get_revision`, `is_current_or_retained`, `resolve_live_file`, `publish` with A's node and parent, `lookup_revisions(A's hashes)`, `find_revisions`; diff A's rows before/after.
**Expected behavior**: `NotFound`/`ParentNotFound`/`Gone`/empty; no write.
**Pass criteria**: 0 successful cross-account calls; A snapshot identical; error values equal to those for random ids.
**Fail criteria**: any distinct error that reveals existence, or any change.

### ST-02-2: Row-level security with the runtime role
**Summary**: as `sunnycloud` scoped to B, raw `SELECT` on `nodes` and `revisions` shows no A rows; maintenance loops use system scope only through helper 07.
**Traces to**: AC-59
**Attack vector**: query without `account_id`; `SET ROLE`; DDL.
**Test procedure**: raw selects under B scope; attempt `ALTER TABLE nodes DISABLE ROW LEVEL SECURITY`; run `cascade_batch` as the janitor and confirm it sees all accounts.
**Expected behavior**: B sees only B rows; DDL denied; janitor sees all.
**Pass criteria**: 0 foreign rows, DDL permission denied (system: NFT-SEC-46). **Fail criteria**: any foreign row.

### ST-02-3: Hostile names and injection
**Summary**: SQL metacharacters, LIKE wildcards, path-like and bidi names are data.
**Traces to**: AC-37, AC-59
**Attack vector**: names `"'; DROP TABLE nodes;--"`, `"%"`, `"_"`, `"a\\b"`, `"..%2f"`, 255-byte multibyte; `resolve_path("../x")`, `resolve_path("a//b")`; `after_name_key` cursor with quotes; `find_revisions` device filter with quotes.
**Test procedure**: create with each valid name, list, resolve, page with crafted cursors.
**Expected behavior**: valid names stored verbatim and listed once; `..`-style paths -> `NotFound`; crafted cursor -> `InvalidCursor` or literal match, never an SQL error or broader result.
**Pass criteria**: `nodes` table intact, listings exact. **Fail criteria**: 5xx-class error or extra rows.

### ST-02-4: Foreign blob reference at publish
**Summary**: a `NewRevision.blob_id` of another account must not be attachable.
**Traces to**: AC-59, AC-43
**Attack vector**: publish with A's `blob_id` while authenticated as B.
**Test procedure**: B publishes a `New` entry naming A's blob id (real `BlobCommitEffect`; recording effect).
**Expected behavior**: BLOCKED — the description does not say which layer checks blob ownership in `publish`. Reported as a spec gap.
**Pass criteria**: pending decision. **Fail criteria**: B reads A's bytes through a new node.

## Acceptance Tests

### AT-02-1: Stale edit yields one conflict copy
**Summary**: two devices edit the same base; both versions survive.
**Traces to**: AC-8
**Preconditions**: file `report.docx` r1 on devices laptop and phone.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | laptop publishes r2 (base r1) | current = r2 |
| 2 | phone publishes r3 (base r1) | current = r3 |
| 3 | list folder | `report.docx` (r3) and exactly one `report (conflict copy YYYY-MM-DD HH-MM phone).docx` holding r2 |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-02 | listing-10k folder | generated (`gen_listing.py`), loaded by batch `publish` once into the template DB `tmpl_listing` | about 10 MiB |
| DS-23 | small fixtures (2 MiB-1, 2 MiB, 2 MiB+1 sizes) | generated | small |
| Tree factory | builds nested trees, accounts through 01 hooks | code | n/a |
| Recording / failing / replay effects | `CommitEffect` test implementations | code | n/a |

**Setup procedure**: (1) one `postgres:18@sha256:<pin>` per test binary; `sunnycloud migrate` as `sunnycloud_migrator` into `tmpl`; a second template `tmpl_listing` with DS-02 loaded; (2) per test `CREATE DATABASE t_<uuid> TEMPLATE tmpl` (or `tmpl_listing`); component connects as `sunnycloud` (RLS on); probes as `sunnycloud_migrator`; (3) blob volume: per-test temp directory; manual `Clock`; config durations in ms (`tombstone_horizon`, retention, batch sizes small).

**Teardown procedure**: drop per-test database and temp dir; container removed at binary end; connection-count baseline check.
**Data isolation strategy**: database per test from a migrated template; concurrency tests need several real connections so transactions are not the isolation device. DS-02 template is read-only shared. No shared mutable state between tests.
