# Contract: contracts part 2 — 02 tree, 03 transfer, 05 sync_feed ports and the commit-effect protocol

**Component**: `contracts` (server-only port crate, E-00)
**Producer task**: E-00/c04 — `_docs/02_tasks/todo/B01-13_contracts_part2.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-00/c05 (part 3); other batches: E-11..E-14, E-16..E-18, E-21, E-22, E-30
**Version**: 1.0.0
**Status**: draft (frozen when the producer task is accepted)
**Last Updated**: 2026-11-04

## Purpose

The ports of the commit path and the change feed: the tree (02, owner of the commit transaction), transfer (03), the sync feed (05) and the `CommitEffect` protocol that 03 and 08 implement inside 02's transaction. It also declares the four blob data-plane value types that 03's signatures need (`BlobRef`, `ByteStream`, `PlaintextStream`, `ByteRange`); they stay owned by 04 for later changes (module-layout Rule 6) but are declared here so part 2 compiles before part 3.

Conventions (ids, `Timestamp`, `DbTx`, error enums, object safety) are those of part 1 (`part1_registries_identity_admin_quota.md` § Conventions). Sources: `components/02_tree/description.md` §2 and Ports, `components/03_transfer/description.md` §3 and Ports, `components/05_sync_feed/description.md` §2 and Ports, `components/04_blobstore/description.md` §3, `canonical-names.md` § Ports, `data_model.md` §5, §7.

## Shape

### Blob data-plane value types (owned by 04)

| Type | Shape |
|---|---|
| `BlobRef` | `blob_id: Uuid, size: i64, wrapped_key: bytes, master_key_id: i16` (types follow data_model §6.1; 04 §3: defined once; media type is not part of it) |
| `ByteStream` | neutral `Stream<Item = Result<Bytes, StreamError>>` (request bodies; no framework type) |
| `PlaintextStream` | the same item type, produced by 04's reader; a failed tag yields `Err(Corrupt)` and ends, never a short body |
| `ByteRange` | one inclusive byte range `start..=end` |

### 02 tree

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `TreeService` | `create_folder` | `account, parent_id, name` -> `NodeView` | `NameInvalid(reason)`, `NameConflict`, `ParentDeleted`, `NotFound`, `NotAFolder` |
| | `update_node` | `account, node_id, new_name?, new_parent?` -> `NodeView` | `NameInvalid`, `NameConflict`, `Cycle`, `ParentDeleted`, `NotAFolder`, `IsRoot`, `NotFound` |
| | `delete_node` | `account, node_id, base_revision?` -> `()` (idempotent) | `EditedSince{current_revision}`, `IsRoot`, `NotFound` |
| | `resolve_path` | `account, path` -> `Uuid` | `NotFound` |
| | `find_revisions` | `account, client_device filter, page` -> `Vec<RevisionRef>` | `InvalidCursor` |
| `TreeReader` | `get_node` | `account, NodeSelector{Id(Uuid) \| Root}` -> `NodeView` | `NotFound` |
| | `list_children` | `account, folder_id, after_name_key?, limit` -> `ListingPage{items, next}` | `NotFound`, `NotAFolder`, `InvalidCursor` |
| | `find_live_node` | `account, parent_id, name` -> `Option<NodeRef>` | none |
| | `resolve_live_file` | `account, node_id` -> `LiveFileLookup{Found(LiveFile) \| Gone}` | none |
| | `root_node` | `account` -> `NodeRef` | none |
| | `get_revision` | `account, revision_id` -> `RevisionView` (with blocklist) | `NotFound` |
| | `is_current_or_retained` | `account, revision_id` -> `bool` | none |
| | `describe_blobs` (system scope) | `&[Uuid]` blob ids -> map blob id -> node paths | none |
| | `lookup_revisions` | `account, &[FileHash]` (at most 1000) -> `Vec<RevisionMatch{hash, node_id, revision_id, client_meta}>` | `BatchTooLarge` |
| `TreeCommit` | `publish` | `CommitActor, Vec<PublishEntry>, Vec<Arc<dyn CommitEffect>>, now: Timestamp` -> `PublishResult{Published(Vec<EntryResult>) \| Replayed(StoredResponse)}` | whole call: `AccountNotActive`, `CommitIdReused`, `EffectFailed(ErrorCode)` |
| `TreeRestore` | `restore_revision` | `account, NodeOrPath, at: Timestamp` -> `RestoreOutcome{restored_revision, placement: Original \| Restored}` | `BeyondRetention`, `FileMissing`, `NotFound` |
| `LiveUsageSource` | `live_bytes` | `account` -> `i64` | none |
| `TreeMaintenance` | `cascade_batch` | `now, limit` -> `processed: u64` | `Unexpected` |
| | `purge_tombstones` | `now, limit` -> `processed: u64` | `Unexpected` |

Per-entry failures of `publish` (an `EntryResult` value, not an error of the call): `NameInvalid`, `ParentDeleted`, `ParentNotFound`, `NotAFolder`, `NotFound`, `BlocklistMismatch` (blob not owned by the account, 02 Ports), `EffectFailed(ErrorCode)`.

| DTO | Fields / variants |
|---|---|
| `CommitActor` | `account_id, device_label` |
| `PublishEntry` | `target: Target, revision: NewRevision, vault: bool` (false in v1.0) |
| `Target` | `Existing{node_id, base_revision?}`, `New{parent_id, name}` |
| `NewRevision` | `revision_id, blob_id, size, content_hash: FileHash, blocklist: Vec<Hash32>, media_type?, client_mtime?, client_meta?, reserved_bytes, import_hint` |
| `EntryResult` | `Published{outcome: PublishOutcome, node_id, revision_id, conflict_copy?}` or a per-entry failure |
| `PublishOutcome` | `Created`, `Updated`, `Replaced`, `ConflictCopied{copy_node}`, `CreatedAsCopy{node}`, `Revived`, `NoChange{existing_revision}` |
| `LiveFile` | `kind: File \| Folder`, `revision: Option<RevisionView>` (current), `blob_ref: Option<Uuid>` (the revision's blob id; empty for a folder) |
| `NodeView` | derived from data_model §5.1/§5.2: `id, parent_id, kind, name, state: Live \| Tombstone, current_revision_id?, size?, content_hash?, media_type?, client_mtime?, created_at, updated_at, seq`, plus the thumbnail state field that E-13 adds from `ThumbnailStateSource` |
| `NodeRef` | `id, kind` |
| `RevisionView` | `id, node_id, size, content_hash, blocklist?, media_type?, client_mtime?, client_meta?, created_at` |
| `RevisionRef` | `revision_id, node_id` |

### Commit-effect protocol (implemented by 03 and 08, run inside 02's transaction)

| Method | Signature | Result |
|---|---|---|
| `on_begin` | `(tx: &mut DbTx) -> Result<Begin, EffectError>` | `Begin::Proceed` or `Begin::Replay(StoredResponse)` |
| `on_entry` | `(tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError>` | `Entry` error rolls back that entry's savepoint only |
| `on_batch` | `(tx: &mut DbTx, outcomes: &[EntryOutcome]) -> Result<(), EffectError>` | receives every entry outcome, failed ones included |

`EffectError` = `Entry(ErrorCode)` or `Batch(ErrorCode)` (whole transaction rolls back). `PublishedEntry` = `node_id, revision_id, content_hash, media_type?, size, import_hint, vault`. `EntryOutcome` = the entry's `EntryResult` paired with its index. Effects do no I/O, read only rows they own and add no statement after the feed stamp.

### 03 transfer

All methods take the resolved account id first (from 11's authentication), never a credential.

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `UploadService` | `start_upload` | `account, StartUpload{parent_id, name, declared_size, client_key, content_hash?, base_revision?, client_meta?}` -> `Started{upload_id, need_blocks, block_size, lane_limit, expires_at}` or `NothingToSend{revision}` | `InvalidName`, `ParentNotFound`, `NotAFolder`, `SizeTooLarge`, `QuotaExceeded`, `SessionCapReached`, `FreeSpaceFloor`, `ClientKeyMismatch`, `AccountNotActive` |
| | `get_upload` | `account, upload_id` -> `UploadState{need_blocks, received_count, expires_at}` | `UploadNotFound` |
| | `put_block` | `account, upload_id, index, expected_hash: Hash32, body: ByteStream, length` -> `BlockAck` | `UploadNotFound`, `IndexOutOfRange`, `LengthMismatch`, `HashMismatch`, `BlockConflict`, `Overloaded`, `StorageFull`, `Storage` |
| | `abort_upload` | `account, upload_id` -> `()` | `UploadNotFound` |
| | `stage_small` | `account, staged_key, expected_hash?, body: ByteStream, length (0..=2 MiB)` -> `Staged{staged_key, size}` | `QuotaExceeded`, `StagedCapReached`, `HashMismatch`, `LengthMismatch`, `ClientKeyMismatch`, `FreeSpaceFloor`, `Overloaded`, `AccountNotActive`, `Storage` |
| `CommitService` | `commit_upload` | `CommitActor, CommitUpload{upload_id, parent_id, name, base_revision?, blocklist, client_mtime?, client_meta?, import_hint}` -> `Committed{node_id, revision_id, conflict_copy?}` | `NeedBlocks(list)`, `BlocklistMismatch`, `UploadNotFound`, `InvalidName`, `EditedSince`, `ParentGone`, `ParentNotFound`, `NotAFolder`, `AccountNotActive` |
| | `commit_batch` | `CommitActor, BatchCommit{commit_id, entries}` (entry `{staged_key, parent_id, name, base_revision?, client_mtime?, client_meta?, import_hint}`) -> `BatchResult{per_entry: Committed \| EntryError}` | `BatchTooLarge`, `CommitIdReused`, `AccountNotActive` |
| `DownloadService` | `open_revision_content` | `account, RevisionOrCurrent{Revision(revision_id) \| Current(node_id)}, retained_ok, range: Option<ByteRange>, if_range?, if_none_match?` -> `Download` | `NotFound`, `RangeNotSatisfiable`, `ContentUnavailable`, `Storage` |
| | `open_blob_content` | `account, BlobRef, media_type?, same options` -> `Download` | as above |
| `ReservationSource` | `open_reserved_bytes` | `account` -> `i64` | none |

`Download` = `status: Full \| Partial \| NotModified, total_size, range?, etag, media_type?, disposition: Inline \| Attachment, suggested_name, body: PlaintextStream`.

### 05 sync_feed

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `FeedStamp` | `stamp` | `tx: &mut DbTx, account, TouchedNodes` (non-empty) -> `Seq` | `AccountFeedMissing` |
| `FeedNotifier` | `notify` (`sync`, non-blocking) | `account, seq` -> `()` | none |
| `FeedHorizon` | `raise_purge_horizon` | `tx: &mut DbTx, account, seq` -> `()` (0 rows is not an error) | none |
| `ChangeFeed` | `read` | `&Principal, &dyn PrincipalRevalidator, Cursor{epoch, seq}, wait?, limit?` -> `ChangePage{items: Vec<ChangeItem>, next_cursor, has_more}` | `ResyncRequired{reason: EpochChanged \| BelowPurgeHorizon \| CursorAhead}`, `Unauthenticated`, `BadCursor`, `TooManyHolds{retry_after}` (PROPOSED, additive; E-12 / E-00; G-05-2 decided 2026-11-07; 11 maps it to the existing `rate_limited` 429 with `Retry-After`) |
| | `head` | `account` -> `Cursor` | none |
| `EpochAdmin` | `bump_epoch` | `now: Timestamp` -> `new_epoch: i64` | `Unexpected` (includes a failed marker write after commit) |
| | `marker_status` | `()` -> `MarkerStatus{db_epoch, marker: Absent \| Present(epoch)}` | none |
| | `head_report` | `()` -> `HeadReport{max_epoch, heads: per-account Cursor}` | none |

`ChangeItem` = `node_id, parent_id, kind, name, state: Live \| Tombstone, revision_id?, size?, content_hash?, seq` (latest state of one node, not an event).

## Invariants

- `TreeCommit::publish` is the only way a revision becomes current; the caller supplies its effects per call and never sees the transaction.
- `FeedStamp::stamp` is the last statement of a tree-changing transaction; `notify` is called only after COMMIT. A page of `ChangeFeed::read` ends on a `seq` boundary.
- A folder and a gone node are distinct results of `resolve_live_file` (`kind = Folder` versus `Gone`), never told apart by an error.
- Retained non-current revisions are served only when `retained_ok` is true (admin principal, set by 11).
- `BlobRef` has exactly the four fields above; nothing else in the crate redefines it.
- All conventions of part 1 hold (no implementation, no HTTP/sqlx types, `Arc<dyn Port>` usable, `DbTx` only from `sc_db`).

## Non-Goals

- Not covered: the commit algorithm, conflict-copy naming, cursor checks, hold algorithm, sniffing, upload buffering (component epics E-12..E-14).
- Not covered: 04 ports (part 3); HTTP mapping of the variants (11).

## Versioning Rules

- Breaking changes need a new major version and a `contracts` change task in the provider's epic; `CommitEffect` changes also notify 03 and 08.
- New optional fields, a new `PublishOutcome` variant only after every caller handles it (that is breaking), new error variants callers treat as unexpected: minor.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| listing-part2 | public traits after part 2 | every port of parts 1 and 2 present, 0 missing | |
| effect-fake | a test `CommitEffect` returning `Entry(code)`, `Batch(code)`, `Replay` | compiles; holdable as `Arc<dyn CommitEffect>` in the `publish` signature | |
| outcome-variants | exhaustive match on `PublishOutcome` and `EffectError` in a test | 7 and 2 variants, no wildcard needed | catches drift |
| blobref-single | search the crate for a second `BlobRef` definition | none | 04 owns it |
| tx-only-where-listed | compile test: `stamp`, `raise_purge_horizon`, effect methods take `&mut DbTx` | compiles; other methods take none | |

## Open points (flagged in the producer task)

1. 03 lists `media_type` inside `BlobRef`; 04 (the owner) says the media type is not part of `BlobRef`. This contract follows 04 and adds `media_type?` as a separate `open_blob_content` parameter.
2. `LiveFile.blob_ref` is the blob id (02 §2), not a `BlobRef` struct, although AZ-3652's DTO table uses the same word.
3. 08 describes `on_batch(tx, &[PublishedEntry])` and an input `CommitContext{account_id, ...}`; 02 (owner) defines `on_batch(tx, &[EntryOutcome])` and `PublishedEntry` without `account_id`. 08's job key needs the account id: either `PublishedEntry` gains `account_id` or the effect reads it from `CommitActor`. Owner of the commit transaction (E-13) to decide.
4. `StoredResponse` (the receipt's stored batch response replayed by `Begin::Replay`) has no specified type; it must be the serialised `BatchResult` of 03.
5. `ChangeFeed` "also takes a shutdown signal" (05 §2) without saying how; this contract leaves it to construction by 12 (not a method).
6. `NodeView`, `RevisionView` and `ListingPage` field lists are derived from data_model §5 and 05's `ChangeItem`; 02 does not list them. E-13 may add fields additively.
7. `TouchedNodes`, `Seq`, `HeadReport` are named but not shaped beyond the obvious (a non-empty set of node ids; an `i64`; max epoch and per-account heads).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
