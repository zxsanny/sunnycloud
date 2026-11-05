# Contract: commit-service (CommitService behaviour and transfer commit effects)

**Component**: 03_transfer
**Producer task**: E-14/c08 — `_docs/02_tasks/todo/B03-24_transfer_commit_upload.md` (contributor: E-14/c09 `B03-25_transfer_commit_batch.md`; response mapping decided by spike E-14/c01); keys assigned at Step 4
**Consumer tasks**: E-13/c16, E-14/c12, E-14/c15, E-14/c16 (this batch); E-21/c02, E-21/c13 (11), E-22/c03 (composition root), E-23/E-26/E-27 through the HTTP API, resolved at Step 4
**Version**: 1.0.0
**Status**: draft (frozen when E-14/c01 has recorded G-03-3 and the producer task passes review)
**Last Updated**: 2026-11-04

## Purpose

Turning uploaded or staged blobs into revisions: single large commits and idempotent batches of up to 500 staged files. 03 never opens the commit transaction; it calls `TreeCommit::publish` (`../02_tree/tree_commit.md`) with its own effects. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 03 transfer).

## Shape

Signatures: part 2 (`CommitService`). Behaviour:

| Method | Behaviour | Errors |
|---|---|---|
| `commit_upload(actor, CommitUpload)` | Outside any transaction: blocklist against received hashes, whole-file hash from block hashes, `read_head` (<= 64 KiB) and byte sniffing. Target: `Existing` on the node of `base_revision`, else `New{parent_id, name}`. One `publish` call with `BlobCommitEffect` plus injected effects | `NeedBlocks(list)`, `BlocklistMismatch`, `UploadNotFound`, `InvalidName`, `EditedSince` (open: G-03-3), `ParentGone`, `ParentNotFound`, `NotAFolder`, `AccountNotActive` |
| `commit_batch(actor, BatchCommit)` | Up to `batch_max_entries` (500) staged entries in one `publish` call with `ReceiptEffect`, `BlobCommitEffect` and injected effects | `BatchTooLarge`, `CommitIdReused`, `AccountNotActive`; per entry: `EntryError` |

Effects supplied by 03:

| Effect | Hook | Behaviour |
|---|---|---|
| `ReceiptEffect` (batch only) | `on_begin` | Look up `commit_receipts(account, commit_id)` under the lock `publish` holds: same `request_hash` gives `Replay(stored response)`; different gives `Batch(CommitIdReused)` |
| `ReceiptEffect` | `on_batch` | Insert `(account, commit_id, request_hash, response)` with every entry outcome |
| `BlobCommitEffect` | `on_entry` | Lock the entry's session or staged row (`FOR UPDATE`); zero rows gives `Entry(UploadNotFound)`; otherwise mark the blob committed and delete the row |

Outcome mapping to `Committed{node_id, revision_id, conflict_copy?}` (draft until G-03-3 is decided):

| `PublishOutcome` | `node_id` | `conflict_copy` |
|---|---|---|
| `Created`, `Updated`, `Replaced`, `Revived` | the committed file | absent (`Revived` with a taken name: open) |
| `ConflictCopied{copy_node}` | the committed file | the copy holding the superseded content |
| `CreatedAsCopy{node}` | the new file, which carries the conflict-copy name | open |
| `NoChange{existing_revision}` | the file | absent; `revision_id` = existing revision |

Entry-error mapping: 02 `ParentDeleted` → `ParentGone` (`parent_deleted` 410); 02 `NameInvalid` → `InvalidName`.

## Invariants

- Commit time does not depend on file size: no file bytes are read beyond the head (AC-4).
- The media type comes from bytes only; client type and file name are ignored; unknown is no type (served as attachment).
- `name_conflict` never occurs on commit.
- `parent_id` is only a hint for an existing file; the node is addressed by id (AC-70).
- Receipts exist for batches only; the stored response is the serialised `BatchResult`; receipts are per account and live `commit_receipt_ttl` (48 h).
- An identical batch applies at most once, also under concurrency, without relying on a unique violation.
- 03's effects add no statement after the stamp and do no I/O (AC-69).

## Non-Goals

- Not covered: the transaction itself, conflict rules, quota deltas (`../02_tree/tree_commit.md`); HTTP status choice for `need_blocks`/`edited_since` (E-21/c02).

## Versioning Rules

- **Breaking changes** (response fields, receipt semantics, error removal) require a new major version and a `contracts` change task in E-14.
- **Non-breaking additions** require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| roll-up-only | 1 MiB and 8 GiB commits with a reader decorator | 1 head read each; time within 20% | IT-03-6, PT-03-2 |
| stale-base | commit on a stale base | `Committed` with the conflict copy; no `EditedSince` | IT-03-7 (pending G-03-3) |
| replay | identical batch resent | stored response; nothing applied | IT-03-8 |
| id-reused | same `commit_id`, changed entry | `CommitIdReused` | IT-03-8 |
| reaped-entry | session removed before publish | `UploadNotFound`; reservation released once | IT-03-9 |

## Open items

- G-03-3: fields for `CreatedAsCopy` and a revived file with a taken name, and whether `EditedSince` stays (E-14/c01, owner).
- `request_hash` canonical form (entry order, optional fields).
- Pipeline AZ-3656 expects 413 for an oversized batch; 03 maps `batch_too_large` to 422.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
