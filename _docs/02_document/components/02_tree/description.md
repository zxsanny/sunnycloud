# 02_tree — File tree, revisions, conflicts, tombstones, commit transaction

## 1. High-Level Overview

**Purpose**: Own the per-account tree of folders and files (`nodes`, `revisions`): names and uniqueness, create/rename/move/delete, publishing a revision with the "later commit wins / conflict copy / edit beats delete" rules, tombstones, cascade and purge, listing and revision lookup. Own the commit transaction.

**Single responsibility**: the shape and history of the tree, and the one transaction that publishes a change to it. It does not move bytes (03_transfer), does not own blobs (04_blobstore; referenced by id), does not own the counter (05_sync_feed), usage numbers (07a_quota), jobs (08) or shares (06).

**Architectural pattern**: command/query service. Every tree-changing operation opens its own short transaction through helper 07 and ends it the same way: node and quota writes, caller-supplied `CommitEffect`s, `FeedStamp::stamp` as the last statement (skipped when nothing was touched), COMMIT, then `FeedNotifier::notify`. No other component writes `nodes` or `revisions` or stamps the feed.

**Scope in**: name rules via helper 06 (`NameKey`), listing with keyset paging, revision metadata (including per-revision `client_meta`), conflict-copy creation, revive-on-edit, folder cascade and tombstone purge batches, operator restore of a revision, path resolution for the CLI, hash and device lookup of revisions, the quota deltas of a commit, the account-erase hook for tree rows.
**Scope out**: upload sessions, receipts, staged blobs, bitmap checks (03); blob state flip and `blob_gc` writes (04, through ports); feed reads (05); share records (06 reads this component through `TreeReader`); thumbnails (08).

**Upstream dependencies**: helpers (names, ids, clock, error, db) and `contracts` ports only (see Ports). No component crate.
**Downstream consumers**: through ports only; see Ports.

## 2. Internal Interfaces

### TreeService (mutations and path/device queries; own short transactions, all scoped by `account_id`)
| Method | Input | Output | Errors (typed) |
|---|---|---|---|
| `create_folder` | account, parent id, name | `NodeView` | `NameInvalid(reason)`, `NameConflict`, `ParentDeleted` (410), `NotFound`, `NotAFolder` |
| `update_node` | account, node id, optional new name, optional new parent | `NodeView` | `NameInvalid`, `NameConflict`, `Cycle` (409), `ParentDeleted`, `NotAFolder`, `IsRoot`, `NotFound` |
| `delete_node` | account, node id, `base_revision` (files) | `()` idempotent | `EditedSince{current_revision}`, `IsRoot`, `NotFound` |
| `resolve_path` | account, path text | node id | `NotFound` |
| `find_revisions` | account, filter `client_device`, paging | `[RevisionRef]` | `InvalidCursor` |
`update_node` carries no `base_revision`: rename and move never conflict with content, only with a name (`NameConflict`). A case-only rename keeps the node id. Creating under a tombstoned parent gives `ParentDeleted`; under a missing id `NotFound`; under a file `NotAFolder`. Restore is the separate `TreeRestore` port below.

### TreeReader (reads; also the former `NodeResolver`)
| Method | Output | Notes |
|---|---|---|
| `get_node(account, id or Root)` | `NodeView` | `NotFound`; a node under a tombstoned ancestor is reported deleted at once (same parent-chain walk), as are the node lookups 03, 06, 08 and 11 rely on |
| `list_children(account, folder, after_name_key?, limit)` | `ListingPage{items, next}` | default limit 1000; `NotFound`, `NotAFolder`, `InvalidCursor` |
| `find_live_node(account, parent, name)` | `Option<NodeRef>` | by `name_key`, live only |
| `resolve_live_file(account, node_id)` | `LiveFile{kind, revision, blob_ref}` or `Gone` | `kind` is `File` or `Folder`; for a folder `revision` and `blob_ref` are empty, so callers tell "is a folder" from "gone" (tombstoned, purged, other account, or **any ancestor folder tombstoned**: the lookup walks the parent chain, bounded by depth, so it does not wait for the background cascade). `revision` is the current `RevisionView` (size, media type, content hash); `blob_ref` is the revision's `blob_id` (03 resolves key material through `BlobCatalog`) |
| `root_node(account)` | `NodeRef` | e.g. `root_node_id` in 07's account view |
| `get_revision(account, revision_id)` | `RevisionView` (incl. blocklist) | `NotFound` |
| `is_current_or_retained(account, revision_id)` | `bool` | 08's lease check; users are never served non-current revisions (03) |
| `describe_blobs(blob_ids)` | paths of the nodes that use each blob | system scope, for `doctor` missing-blob report (AC-73) |
| `lookup_revisions(account, hashes)` | `[RevisionMatch{hash, node_id, revision_id, client_meta}]` | at most 1000 hashes (`BatchTooLarge` above), current revisions of live files; backs `POST /api/v1/revisions:lookup` (reinstall rebuild, AC-46). No commit-by-hash exists |

### TreeCommit (owns the commit transaction)
| Method | Input | Output | Errors |
|---|---|---|---|
| `publish` | `CommitActor{account_id, device_label}`, `Vec<PublishEntry>` (one for a single commit, up to the batch limit otherwise), `Vec<Arc<dyn CommitEffect>>`, `now` | `Published(Vec<EntryResult>)` or `Replayed(stored response)` (a receipt effect found the commit id already applied). Entry result: `Published{outcome, node_id, revision_id, conflict_copy?}` or a typed entry failure | whole call: `AccountNotActive`, `CommitIdReused` (from an effect), `EffectFailed` (batch effect); per entry `NameInvalid`, `ParentDeleted`, `ParentNotFound`, `NotAFolder`, `NotFound`, `EffectFailed(code)` |
`PublishEntry` = `Target::Existing{node_id, base_revision?}` or `Target::New{parent_id, name}`, `NewRevision{revision_id, blob_id, size, content_hash, blocklist, media_type, client_mtime, client_meta, reserved_bytes, import_hint}`, `vault: bool` (false in v1.0; 08 skips media jobs for vault files). The committing device label comes from `CommitActor`, not from the entry. `PublishOutcome` = `Created`, `Updated`, `Replaced`, `ConflictCopied{copy_node}` (stale base), `CreatedAsCopy{node}` (new file whose name was taken: the server gives the new file the conflict-copy name), `Revived`, `NoChange{existing_revision}`. A new-file name collision is never an error and the client does not retry.

Sequence inside the one transaction: `AccountGuard::ensure_active(tx, account)` first (`AccountNotActive` for the whole call) -> `advisory_lock_account` -> every effect's `on_begin` (a receipt effect looks the commit id up here, under the lock, so two identical batches cannot both pass and no unique violation is needed; a hit returns `Replayed` and nothing else runs) -> for each entry, in its own savepoint: node and revision writes, `QuotaLedger::adjust_used(delta)` and `QuotaLedger::release_reservation(reserved_bytes)`, `GcQueue::schedule_in_tx` for a replaced blob, then every effect's `on_entry` -> after all entries every effect's `on_batch(tx, &[EntryOutcome])` with the outcome of every entry, failed ones included (so a receipt can store the whole response) -> `FeedStamp::stamp` with the union of touched nodes (one `seq` for the batch) -> COMMIT -> `FeedNotifier::notify`. When no entry touched a node (all failed, or all `NoChange`) there is no stamp and no notify; the receipt still commits. A typed failure rolls back that entry's savepoint including its effects and leaves the transaction usable; the other entries commit. Nothing runs between `stamp` and COMMIT (AC-69). Every upload commit (03) uses this path; restore has its own transaction with the same tail.

### Locking and retries
The account advisory lock is taken by `publish` (single and batch), by `update_node` when it moves a node (then a recursive cycle check, depth about 30) and by restore. `create_folder`, `delete_node`, `cascade_batch`, `purge_tombstones` and the erase hook do not take it; they work in short batches and rely on the unique name index and row locks. Creating a child or a revision locks the parent row `FOR SHARE` and `delete_node` updates it, so nothing is added under a folder whose tombstone commits first. There is no lock-order contract between components; a transaction the database aborts for deadlock or serialization is retried once by the operation, then fails as unexpected. `stamp` stays the last statement.

### Other operations that stamp
`create_folder`, `update_node`, `delete_node` call `AccountGuard::ensure_active` first and run the same tail (quota delta for delete, `stamp`, COMMIT, `notify`) in their own transaction.

### Narrow traits this component implements
- `TreeRestore::restore_revision(account, node id or path, instant at) -> RestoreOutcome{restored_revision, placement: Original | Restored}` (`BeyondRetention`, `FileMissing`, `NotFound`; operator only, used by `admin restore`). Decision 4. One transaction under the advisory lock: pick the latest revision with `created_at <= at`; `BlobRegistry::reinstate_in_tx(tx, blob_id)` (clears `blob_gc`, `gc_pending` to `committed`; required port, a refusal rolls everything back); then **re-point the existing revision row**, never insert a second one (`revisions.blob_id` is UNIQUE). If its original folder is live (`Original`): the original node (revived if tombstoned; a name taken meanwhile gets the conflict-copy name) gets `current_revision_id = restored`; a previously current revision is queued with `GcQueue::schedule_in_tx(Replaced)`. Otherwise (`Restored`): a file node in a root folder named `Restored` (created on first use) is created and the revision row is re-parented to it (`UPDATE revisions SET node_id`). Quota delta (+restored size, minus the replaced current size), `stamp` over the touched nodes, COMMIT. The CLI runs in another process, so `notify` reaches nobody; held polls in the API process see the change on 05's 10 s recheck.
- `LiveUsageSource::live_bytes(account) -> i64`: sum of the sizes of the current revisions of live files, for the caller of 07a's `recompute` (AC-74).
- `TreeMaintenance`: `cascade_batch(now, limit) -> processed`, `purge_tombstones(now, limit) -> processed` (calls `FeedHorizon::raise_purge_horizon` as the last statement of its transaction). Each is restart-safe by query alone and is also wrapped as a `Janitor` for the scheduler.
- `AccountLifecycleHook`: `initialize(tx, account)` inserts the account root node. `erase_batch(account, limit) -> EraseProgress{remaining}` works in short transactions without the advisory lock: deletes revision rows and queues each blob with `GcQueue::schedule_in_tx(tx, blob, AccountDeleted, Due::Now)` in the same transaction as the row delete, then nodes leaf first, the root last. `remaining` counts nodes, revisions and the account's blobs still queued in `blob_gc` (`GcQueue::pending_for_account`), so 07 finalizes only when the sweeper has removed the files.
- `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)` (called by the GC sweeper in 04): deletes the revision row that points at the blob **whatever its state**, current or not. The FK `ON DELETE SET NULL` clears `nodes.current_revision_id` of a tombstoned file, so `revisions.blob_id RESTRICT` never blocks the collection of a deleted file's blob. The list of cleanup implementers is registered late by 12.

### Narrow traits this component uses
- `ThumbnailStateSource::states(account, [content_hash]) -> map` giving `ready|pending|failed|none` (implemented by 08); called once per listing page for image/video items only (confirmed answer 5). If no implementation is wired, items carry `none`.
- `CommitEffect` (defined in `contracts`; implemented by 03 and 08, supplied by the caller of `publish`): `on_begin(tx) -> Result<Begin, EffectError>` (`Proceed` or `Replay(response)`), `on_entry(tx, &PublishedEntry) -> Result<(), EffectError>` and `on_batch(tx, &[EntryOutcome]) -> Result<(), EffectError>`. `EffectError` is `Entry(code)` (that entry's savepoint rolls back, the others commit) or `Batch(code)` (the whole transaction rolls back). `PublishedEntry` carries node, revision, content hash, media type, size, `import_hint`, `vault`. Effects run in the commit transaction, perform no I/O and no reads beyond the rows they own, and add no statement after the stamp.

## Ports (crate `contracts`)
| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `TreeService` | 11, 12 | folder create, rename/move, delete, `resolve_path` for the CLI, device filter |
| Provides | `TreeReader` (incl. former `NodeResolver`) | 03, 06, 07, 08, 11, 12, 20 | node and listing reads, existing-target and parent checks, share target (`resolve_live_file`), current-or-retained check, root node, revision lookup, doctor blob paths |
| Provides | `TreeCommit` | 03 | upload commit and batch commit |
| Provides | `TreeRestore` | 12 | `admin restore` (`placement: Original \| Restored`) |
| Provides | `LiveUsageSource` | 12, 07 (callers of `QuotaAdmin::recompute`) | `live_bytes` |
| Provides | `TreeMaintenance` (`cascade_batch`, `purge_tombstones`), `Janitor` | 12 scheduler | cascade, tombstone purge |
| Provides | `AccountLifecycleHook` impl | 01 (initialize), 07 (erase_batch) | root node at account creation; tree rows at deletion |
| Provides | `BlobOwnerCleanup` impl | 04 | revision rows at GC |
| Requires | `AccountGuard::ensure_active` | 01 | first statement of `publish`, `create_folder`, `update_node`, `delete_node` (inside their short transactions) |
| Requires | `BlobCatalog`/`BlobRef` account scope | 04 | `publish` checks that `NewRevision.blob_id` belongs to the account through the composite foreign key `revisions (blob_id, account_id) → blobs (id, account_id)` inside the entry's savepoint, not through `BlobCatalog::get_ref` (which answers `None` for an `uploading` blob, the state every large upload commits, and takes no transaction; AZ-3712, 2026-11-13); a foreign or unknown blob gives the existing `BlocklistMismatch` or `NotFound` entry error |
| Requires | `QuotaLedger` (`adjust_used`, `release_reservation`) | 07a | usage in the commit transaction; 03 reserves, 02 releases inside the commit |
| Requires | `FeedStamp`, `FeedNotifier`, `FeedHorizon` | 05 | counter, wake, purge horizon |
| Requires | `GcQueue` (`schedule_in_tx`, `pending_for_account`), `BlobRegistry::reinstate_in_tx` | 04 | removal of replaced and deleted blobs; restore; erase progress |
| Requires | `ThumbnailStateSource` | 08 | listing enrichment |
| Requires | `CommitEffect` (supplied per call) | 03, 08 | receipt lookup, blob state, session row, receipts, job enqueue |

### Conflict and delete rules realised here
- Commit on a node with base = current: new revision becomes current; the replaced revision keeps its row, its blob is queued with `GcQueue::schedule_in_tx(tx, blob, Replaced, Due::AfterRetention)`.
- Base stale (`Target::Existing`): the new revision still becomes current; the revision that was current is re-parented (same revision row, same blob, no copy of bytes) to a new sibling node `name (conflict copy YYYY-MM-DD HH-MM device)` (stem truncated to fit 255 bytes, numeric suffix on collision). Time is the injectable clock; `device` is `CommitActor.device_label` (decision 15), i.e. the device whose commit lost the race. Both nodes are touched.
- New file whose name is taken (`Target::New`): the server names the new file `name (conflict copy YYYY-MM-DD HH-MM device)` by the same rules and answers `CreatedAsCopy`; there is no `name_conflict` on commit and no client retry.
- Commit on a tombstoned file: node and its tombstoned ancestors up to the first live one are revived (no conflict copy), and `cascade_pending` is cleared on every revived ancestor; descendants already marked by cascade stay deleted. All revived nodes are passed to `stamp`. A revived folder therefore turns from tombstone to live in the feed and clients re-list it (05). A name taken meanwhile gives a conflict-copy name.
- Delete with `base_revision` stale: `EditedSince`; delete of a tombstoned node succeeds; folder delete writes one tombstone and `cascade_pending`, never one big transaction.

## 3. External API Specification (resources owned; routed by 11_http_api)
| Endpoint | Method | Scope | Notes |
|---|---|---|---|
| `/api/v1/nodes/root` | GET | SessionOrDevice | the account root |
| `/api/v1/nodes/{id}` | GET, PATCH, DELETE | SessionOrDevice | metadata incl. current revision; PATCH `{name?, parent_id?}` (no `base_revision`); DELETE `?base_revision=` |
| `/api/v1/nodes/{id}/children` | GET | SessionOrDevice | keyset `?after=<name_key>&limit=`; items carry size, mtime, hash, media type, `thumb_state` |
| `/api/v1/nodes` | POST | SessionOrDevice | create folder `{parent_id, name}`; files are created by commits (03) |
| `/api/v1/revisions/{id}` | GET | SessionOrDevice | metadata plus blocklist |
| `/api/v1/revisions` | GET | SessionOrDevice | filter `client_device` (camera-backup reinstall, AC-46); keyset paging |
| `/api/v1/revisions:lookup` | POST | SessionOrDevice | body `{hashes[<=1000]}`; returns `[{hash, node_id, revision_id, client_meta}]` for current revisions of live files (reinstall rebuild, merge check AC-31); colon form is the documented exception like `commits:batch` |
Errors follow the shared problem format (helper 02) with stable codes: `name_conflict` 409, `invalid_name` 422, `cycle` 409, `edited_since` 409, `parent_deleted` 410, `parent_not_found` 404, `not_a_folder` 422, `invalid_cursor` 422, `batch_too_large` 422 (lookup), `not_found` 404 (also for another account's id).

## 4. Data Access Patterns
**Owned tables**: `nodes` (all columns except `seq`, see below), `revisions` including `client_meta jsonb NULL` (decision 11: device, MediaStore asset id, link to original; bounded size CHECK).
**Shared-column contract with 05**: `nodes.seq` and `nodes_feed_idx` belong to 05; tree never writes `seq`, 05's stamp statement is the only writer.
| Query | Freq. | Hot | Index |
|---|---|---|---|
| Listing by parent, ordered by `name_key`, left-join `revisions` by PK | High | Yes | `nodes_parent_name_key` |
| Node/revision by PK, scoped by account | High | Yes | PK, `UNIQUE(id, account_id)` |
| `revisions (account_id, content_hash)` (lookup, 1000 hashes per call) | Medium | No | existing |
| `revisions` by `client_device` | Low | No | expression index `(account_id, (client_meta->>'device'))` on rows with `client_meta` (data model adds it) |
| Cascade / purge scans | Low | No | `nodes_cascade_idx`, `nodes_tombstone_idx` |
Listing must stay an index scan plus PK lookups (CI plan check on DS-02 with the real RLS role). No caching.

## 5. Implementation Details
**Quota deltas computed here** and applied through `QuotaLedger::adjust_used(tx, account, delta)` between node writes and the effects: new file +size; replace +new-old; stale-base commit +new (the superseded revision lives on in the conflict node and still counts); revive +new; delete -current size; cascade batch -sum per batch (AC-74). The reservation taken by 03 at upload start is released in the same transaction by `QuotaLedger::release_reservation(tx, account, reserved_bytes)`, with `reserved_bytes` carried in `NewRevision`. If the entry then fails (for example the upload session was already reaped, `UploadNotFound`), its savepoint rolls back the release too.
**State**: stateless. Folder-delete progress is the `cascade_pending` flag.
**Concurrency**: `UNIQUE(parent_id, name_key) WHERE deleted_at IS NULL` is the arbiter; `DbError::UniqueViolation` becomes `NameConflict` (create, rename) or, on commit, a conflict-copy name. Inside `publish` the check is done in a savepoint, so a violation never aborts the transaction. A commit addresses an existing file by node id, never by path, so a concurrent folder move changes only `parent_id` and the edit keeps its file id (AC-70).
**Libraries**: sqlx 0.9 through helper 07; `unicode-normalization` and case folding live in helper 06 (`name_key_version` pinned there).
**Error handling**: typed `TreeError`, no catch-and-continue; unexpected DB errors go to the central handler.

## 6. Configuration keys
`tombstone_horizon` (90d, shared with 05 for the horizon), `listing_default_limit` (1000), `listing_max_limit` (1000, owner decision 2026-11-13), `cascade_batch_size` (5000), `purge_batch_size` (5000), `tree_maintenance_interval` (30 s; each tick drains cascade and purge until nothing is due within a time budget), `conflict_copy_time_zone` (UTC; the only accepted value, other zones are refused at start, owner decision 2026-11-13). `retention` is read by 04 only. Durations tests override with milliseconds.

## 7. Security notes
Every query carries `account_id`; foreign ids are 404 (AC-59 matrix). Names: server rejects `/`, NUL, `.`/`..`, control and bidi-control characters, over 255 bytes; accepts everything else (AC-37). RLS is defence in depth; maintenance loops use system scope through helper 07. Restore is operator-only (CLI, system scope).

## 8. Dependency graph
Implement after: helpers, `contracts`. The ports it requires are implemented by 01, 04, 05, 07a, 08, and 03/08 implement `CommitEffect`; against `contracts` alone, 02 compiles and tests with those ports backed by the real implementations in the integration suite (no fake stores). Parallel with: 06. Blocks: nothing at compile time; 03, 06, 07, 08, 11, 12 call it through ports. The cleanup and hook lists are registered late by 12. Circularity check: 02's crate depends on helpers and `contracts` only.

## 9. ACs, tests, acceptance
**ACs [release]**: AC-6, AC-8, AC-30 (queues GC), AC-34 (stable node id), AC-37 (server rules), AC-41 (purge horizon with 05), AC-42, AC-27 (purge batch) all [v1.0]; AC-31 and AC-46 server side (hash and device lookup) [v1.0].
**Interaction ACs folded in**: AC-70 (move vs edit: edit lands in moved folder, same id, 0 conflict copies); AC-74 (usage equals sum of live current revisions, with 07a); AC-77 (share follows rename/move because the node id is stable; delete is gone within 5 s through `TreeReader::resolve_live_file`, with 06); AC-68 (tree side: a revision is inserted only in the same transaction in which the blob effect flips the blob committed, so no committed revision references a missing blob; FK `RESTRICT`); AC-69 (one transaction, stamp last). Test IDs for AC-68/69/70/74/77 added in Step 5 (pending).
**Tests**: FT-P-08, 09, 10, 11, 12, 15, 16, 18, 19, 57; FT-N-04, 41; NFT-PERF-10; NFT-RES-09, 18, 25; NFT-SEC-13, 14, 22; FT-P-13/14 (purge horizon). Restore into a deleted folder lands in `Restored` (decision 4).
**Acceptance (observable)**: listing of 10,000 entries p95 at most 300 ms; `A.txt` and `a.txt` never overwrite; stale edit yields exactly one conflict copy named per pattern; delete vs edit leaves the file; case-only rename keeps id; 1,000 concurrent commits/deletes leave `used_bytes` equal to recomputed sum; a failed entry in a 500-entry batch leaves the other entries committed under one `seq`; restore of a deleted file's revision creates no second revision row; a file deleted and collected leaves no revision row and no blob.

## 10. Logging
ERROR unexpected DB failure with ids; WARN cascade batch slower than interval; INFO purge/cascade batch sizes (no names, no paths). Structured JSON.

## 11. Risks
**Defaults chosen at Step 5 gap pass**: (a) a file under a tombstoned ancestor is Gone immediately via the parent-chain walk in `resolve_live_file`/node lookups, independent of the cascade (runs every `tree_maintenance_interval`, 30 s); this keeps the 5 s "missing" bound of AC-12 and AC-77; (b) `create_folder`, `update_node`, `delete_node` also call `AccountGuard::ensure_active`; (c) the blob-belongs-to-account check on `NewRevision.blob_id` is owned by 02 `publish` through 04's `BlobCatalog`/`BlobRef`, with existing error names (`BlocklistMismatch`/`NotFound`).
Thumbnail enrichment adds a second query per page; measure in NFT-PERF-10. Cascade and purge run as janitors in the `sunnycloud` binary (decision 16). Hot-row contention is limited to the tail stamp; a long cycle check must stay before it. A deadlock between a publish and a non-locking batch (cascade, erase) is resolved by the database and retried once. A 500-entry batch holds node locks for the whole transaction; the batch limit bounds it. Publish, move and restore serialise per account on the advisory lock.
