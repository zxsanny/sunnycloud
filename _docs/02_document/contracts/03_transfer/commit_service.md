# Contract: commit-service (CommitService behaviour and transfer commit effects)

**Component**: 03_transfer
**Producer task**: E-14/c08 — `_docs/02_tasks/todo/B03-24_transfer_commit_upload.md` (contributor: E-14/c09 `B03-25_transfer_commit_batch.md`; response mapping decided by spike E-14/c01); keys assigned at Step 4
**Consumer tasks**: E-13/c16, E-14/c12, E-14/c15, E-14/c16 (this batch); E-21/c02, E-21/c13 (11), E-22/c03 (composition root), E-23/E-26/E-27 through the HTTP API, resolved at Step 4
**Version**: 1.5.1
**Status**: frozen for the response mapping (G-03-3 decided 2026-11-11); the producer task still passes review
**Last Updated**: 2026-11-14

## Purpose

Turning uploaded or staged blobs into revisions: single large commits and idempotent batches of up to 500 staged files. 03 never opens the commit transaction; it calls `TreeCommit::publish` (`../02_tree/tree_commit.md`) with its own effects. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` § 03 transfer).

## Shape

Signatures: part 2 (`CommitService`). Behaviour:

| Method | Behaviour | Errors |
|---|---|---|
| `commit_upload(actor, CommitUpload)` | Outside any transaction: blocklist against received hashes, whole-file hash from block hashes, `read_head` (<= 64 KiB) and byte sniffing. Target: `Existing` on the node of `base_revision`, else `New{parent_id, name}`. One `publish` call with `BlobCommitEffect` plus injected effects | `NeedBlocks(list)`, `BlocklistMismatch`, `UploadNotFound`, `InvalidName`, `ParentGone`, `ParentNotFound`, `NotAFolder`, `AccountNotActive` |
| `commit_batch(actor, BatchCommit)` | Up to `batch_max_entries` (500) staged entries in one `publish` call with `ReceiptEffect`, `StagedCommitEffect` and injected effects | `BatchTooLarge`, `CommitIdReused`, `AccountNotActive`; per entry: `EntryError` |

Effects supplied by 03:

| Effect | Hook | Behaviour |
|---|---|---|
| `ReceiptEffect` (batch only) | `on_begin` | Look up `commit_receipts(account, commit_id)` under the lock `publish` holds: same `request_hash` gives `Replay(stored response)`; different gives `Batch(CommitIdReused)` |
| `ReceiptEffect` | `on_batch` | Insert `(account, commit_id, request_hash, response)` with every entry outcome |
| `BlobCommitEffect` (single), `StagedCommitEffect` (batch) | `on_entry` | Lock the entry's session or staged row (`FOR UPDATE`); zero rows gives `Entry(upload_session_gone)` (`UploadNotFound`); otherwise mark the blob committed; a staged row is deleted, a large upload's session row is kept as the committed-key record (`committed_revision_id` = the new revision, `reserved_bytes = 0`, `expires_at = now + upload_session_ttl`; owner decision 2026-11-13) |

Outcome mapping to `Committed{node_id, revision_id, conflict_copy?}` (decided 2026-11-11, AZ-3731, G-03-3). `conflict_copy` is a node id. A client that gets a `conflict_copy` reads the folder again: either the older content sits beside its file, or its own file carries a conflict-copy name:

| `PublishOutcome` | `node_id` | `conflict_copy` |
|---|---|---|
| `Created`, `Updated`, `Replaced`, `Revived` | the committed file | absent (02 never reports `Revived` for a taken name: it reports `CreatedAsCopy`) |
| `ConflictCopied{copy_node}` (stale base) | the committed file, which holds the new content as its current revision | `copy_node`, the sibling named `name (conflict copy ...)` that holds the superseded content |
| `CreatedAsCopy{node}` (name taken) | the new file, which carries the conflict-copy name | `node`, the same id as `node_id`: it means "saved under a conflict-copy name", so the client re-reads the name |
| `NoChange{existing_revision}` | the file | absent; `revision_id` = existing revision |

Entry-error mapping: 02 `ParentDeleted` → `ParentGone` (`parent_deleted` 410); 02 `NameInvalid` → `InvalidName`; 02 `ParentNotFound`, `NotAFolder` as they are. For `commit_upload` (AZ-3738): `EffectFailed(upload_session_gone)` from `BlobCommitEffect` → `UploadNotFound`; 02 `BlocklistMismatch` (blob ownership check) → `UploadNotFound` as well, because 03 publishes its own session's blob, so a missing blob row means abort or the reaper removed it with the session before 02 wrote the revision; 02 `NotFound` (the base revision's node vanished between the lookup and `publish`) → `Unexpected`.

`commit_upload` details (AZ-3738):

- Blocklist check order: a count other than the session's block count → `BlocklistMismatch`; then blocks not received (in-flight ones included) → `NeedBlocks(list)`; then any differing hash → `BlocklistMismatch`.
- A `base_revision` the account has no revision for (unknown or foreign) is ignored, as `start_upload` ignores it: the target is `New{parent_id, name}`.
- `NewRevision.reserved_bytes` is the session's declared size (a large session reserves exactly that until commit).
- `NoChange` (the upload's content already is the file's current revision): `BlobCommitEffect` deletes the session row and the unused blob row instead of keeping a committed-key record; the file is discarded after COMMIT (best effort, logged; a leftover is the orphan sweeper's). The answer is `Committed{node_id, revision_id = existing, no conflict_copy}`.
- Sniffed types (03's own sniffer, owner decision 2026-11-14; JPEG, WebP and MP4 follow 08's output-check rules): `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `image/heic`, `video/mp4`, `video/quicktime`, `video/webm`, `video/x-matroska`, `text/html`, `application/pdf`; anything else has no type. A head that starts like a binary type but carries `<script`, `<html`, `<iframe`, `<svg`, `<body` or `<!doctype html` anywhere (a polyglot) is `text/html` (AZ-3758).
- Metrics: `commits_total{kind = block_list, result = committed | refused | failed}` (refused = a typed error, failed = `Unexpected`), `commit_duration_seconds{kind = block_list}` for every call, `upload_sessions_total{result = committed}`.

`commit_batch` details (AZ-3754):

- More entries than `batch_max_entries` is `BatchTooLarge` before anything is read.
- Outside any transaction: the staged rows the entries name (`client_key` = the staged key's text; open, unexpired, at most one block, every block received), read in one short transaction behind the account guard (`AccountNotActive`); per entry the uploading blob's key material (04 `get_uploading_plan`), the sniffed head (<= 64 KiB) and the target as for `commit_upload`. An entry with no such row, a key named a second time in the batch, or a blob no longer uploading or missing on disk (a concurrent identical batch committed it) is `UploadNotFound` without reaching `publish`; it is stored in the receipt like every other outcome.
- `NewRevision`: `size` = the row's declared size, `reserved_bytes` = the row's `reserved_bytes`, file hash = roll-up of the row's single block hash (no block for a zero-byte file), the sniffed type, and the entry's `client_mtime`, `client_meta`, `import_hint`.
- `request_hash` (owner decision 2026-11-14): BLAKE3 over the entry count and the BLAKE3 hash of each entry's encoding, in the order sent (a reordered batch is a different request). An entry encodes every field: staged key, parent id, length-prefixed name, tagged `base_revision`, `client_mtime` (microseconds), length-prefixed `client_meta`, `import_hint`. The commit id is the receipt key, not part of the hash.
- Stored response (`commit_receipts.response`, jsonb, also the bytes of `Begin::Replay`): an array in entry order of `{"node_id", "revision_id", "conflict_copy"}` or `{"error": "<entry error>"}`; `EffectFailed` carries `{"error": "effect_failed", "code": "<wire code>"}`.
- Entry outcomes: the mapping above; 02 `BlocklistMismatch` and `EffectFailed(upload_session_gone)` are `UploadNotFound`; 02 `NotFound` (the base revision's node vanished between the lookup and `publish`) has no `EntryError` of its own and is `EffectFailed(not_found)`.
- Staged-row effect (`StagedCommitEffect`, `on_entry`): locks the entry's staged row `FOR UPDATE`, marks the blob committed and deletes the row; no open row is `Entry(upload_session_gone)`. The entry is found by its planned revision id; a `NoChange` entry (it carries the file's existing revision) is found by its node and content hash, the first such row still open in entry order: its row and blob row are deleted and the file discarded after COMMIT (best effort, logged).
- Metrics: `commits_total{kind = batch, result}` and `commit_duration_seconds{kind = batch}` for every call (`committed` also when some entries failed).

## Invariants

- Commit time does not depend on file size: no file bytes are read beyond the head (AC-4).
- The media type comes from bytes only; client type and file name are ignored; unknown is no type (served as attachment).
- `name_conflict` never occurs on commit.
- `commit_upload` and `commit_batch` never answer `EditedSince`: 02's `publish` accepts a stale base and answers `ConflictCopied`. `edited_since` stays an error of `delete_node` only. The `EditedSince` variants of `CommitError` and `EntryError` in `contracts` are removed by E-14/c08.
- `parent_id` is only a hint for an existing file; the node is addressed by id (AC-70).
- A committed large upload keeps its `(account, client_key)` → revision until `upload_session_ttl`, so `start_upload` answers a retry after a lost commit response with `NothingToSend{revision}` (`upload_service.md` § `NothingToSend` rule, owner decision 2026-11-13); the reservation is settled at commit as before.
- Receipts exist for batches only; the stored response is the serialised `BatchResult`; receipts are per account and live `commit_receipt_ttl` (48 h); `ReceiptJanitor` deletes older ones, after which the same `commit_id` is a new batch (AZ-3756).
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
| stale-base | commit on a stale base | `Committed{node_id = the file, conflict_copy = the older sibling}`; no `EditedSince` | IT-03-7 |
| name-taken | `Target::New` whose name is taken | `Committed{node_id = n, conflict_copy = n}`, `n` carries the conflict-copy name | IT-03-7 |
| replay | identical batch resent | stored response; nothing applied | IT-03-8 |
| id-reused | same `commit_id`, changed entry | `CommitIdReused` | IT-03-8 |
| reaped-entry | session removed before publish | `UploadNotFound`; reservation released once | IT-03-9 |

## Open items

- `batch_too_large` (more entries than `batch_max_entries`) is 422 (resolved 2026-11-07, G-11-1); a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
| 1.5.1 | 2026-11-14 | The batch runs `StagedCommitEffect`, not `BlobCommitEffect` (as in the `commit_batch` details); client error catalogue and retry rules in `client_protocol.md`; AC-69 statement order covered by IT-03-19 (AZ-3757) | AZ-3757, AZ-3762 |
| 1.5.0 | 2026-11-14 | Polyglot heads sniffed as `text/html` (AZ-3758); receipt expiry by `ReceiptJanitor` (AZ-3756) | AZ-3756, AZ-3758 |
| 1.4.0 | 2026-11-14 | `commit_batch` details: refusals before `publish`, request hash (entries in order sent, owner), stored response form, `NotFound` as `EffectFailed(not_found)`, staged-row effect and `NoChange`, metric labels | AZ-3754 |
| 1.3.0 | 2026-11-14 | `commit_upload` details: blocklist check order, unknown base ignored, ownership failure as `UploadNotFound`, `NoChange` cleanup, sniffed types, metric labels | AZ-3738 |
| 1.2.0 | 2026-11-13 | Committed key kept until `upload_session_ttl` for lost-commit recovery (owner) | AZ-3949 |
| 1.1.0 | 2026-11-11 | G-03-3 decided: fields per `PublishOutcome`; `CreatedAsCopy` repeats the node id in `conflict_copy`; `EditedSince` removed from commit | AZ-3731 |
