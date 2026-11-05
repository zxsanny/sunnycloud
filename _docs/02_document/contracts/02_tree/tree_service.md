# Contract: tree-service (TreeService behaviour)

**Component**: 02_tree
**Producer task**: E-13/c04 — `_docs/02_tasks/todo/B03-04_tree_folder_mutations.md` (contributor: E-13/c12 `B03-12_tree_revision_lookups.md` for `resolve_path` and `find_revisions`); keys assigned at Step 4
**Consumer tasks**: E-13/c09, E-13/c11 (this batch); E-21/c13 (11 handlers), E-22/c10 (`admin restore`), resolved at Step 4
**Version**: 1.0.0
**Status**: draft (frozen when the producer task passes review)
**Last Updated**: 2026-11-04

## Purpose

Folder create, rename, move and delete, plus the path and device queries, as used by 11's node routes and 12's CLI. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`); this file freezes the behaviour.

## Shape

Signatures: part 2 § 02 tree (`TreeService`). Behaviour per method:

| Method | Behaviour | Errors |
|---|---|---|
| `create_folder(account, parent, name)` | Name rules of helper 06; unique per folder by `name_key`; case-preserving | `NameInvalid(reason)`, `NameConflict`, `ParentDeleted`, `NotFound` (missing or foreign parent), `NotAFolder` |
| `update_node(account, id, name?, parent?)` | Rename and move; no `base_revision`; case-only rename keeps the id; a move takes the account advisory lock and runs the cycle check (depth about 30) | adds `Cycle`, `IsRoot` |
| `delete_node(account, id, base_revision?)` | Files: stale base refused with the current revision; tombstoned node: success (idempotent); folders: one tombstone plus `cascade_pending`, descendants marked later | `EditedSince{current_revision}`, `IsRoot`, `NotFound` |
| `resolve_path(account, path)` | Node id of the path; `..`, empty and `.` segments never resolve | `NotFound` |
| `find_revisions(account, client_device, page)` | Revisions whose `client_meta.device` matches, keyset paged | `InvalidCursor` |

Stable codes (helper 02): `name_conflict` 409, `invalid_name` 422, `cycle` 409, `edited_since` 409, `parent_deleted` 410, `parent_not_found` 404, `not_a_folder` 422, `invalid_cursor` 422, `not_found` 404.

## Invariants

- Each mutation is its own short transaction: `AccountGuard::ensure_active` first, writes, `FeedStamp::stamp` last, COMMIT, then `FeedNotifier::notify`.
- Every success advances the account feed by exactly one; every failure consumes no seq.
- Names never collide case-insensitively in one folder (AC-42); the unique index decides, never a server error.
- Nothing is created under a folder whose tombstone committed first.
- `create_folder` and `delete_node` do not take the account advisory lock; a move does.
- A foreign id behaves like a random id (AC-59).

## Non-Goals

- Not covered: file creation and content changes (`tree_commit.md`); restore (`TreeRestore`, CLI only).

## Versioning Rules

- **Breaking changes** (an error removed or renamed, idempotency dropped, a lock added to delete) require a new major version and a `contracts` change task in E-13.
- **Non-breaking additions** (new optional query filter) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| case-collision | `Docs` then `docs` | `NameConflict` | IT-02-1 |
| case-rename | `f.txt` to `F.TXT` | same id, new spelling | IT-02-3 |
| stale-delete | delete with base r1 when current is r2 | `EditedSince{r2}` | IT-02-4 |
| idempotent-delete | delete twice with the current base | `()` both times, one stamp | IT-02-4 |
| traversal-path | `resolve_path("../x")` | `NotFound` | ST-02-3 |

## Open items

- G-1: whether NFC and NFD spellings are one name (E-13/c01, owner).
- Whether `resolve_path` resolves a path whose last node is tombstoned, and which node wins when several tombstones share the path (needed by restore of a deleted file, E-13/c11).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
