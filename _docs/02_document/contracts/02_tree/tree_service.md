# Contract: tree-service (TreeService behaviour)

**Component**: 02_tree
**Producer task**: E-13/c04 — `_docs/02_tasks/todo/B03-04_tree_folder_mutations.md` (contributor: E-13/c12 `B03-12_tree_revision_lookups.md` for `resolve_path` and `find_revisions`); keys assigned at Step 4
**Consumer tasks**: E-13/c09, E-13/c11 (this batch); E-21/c13 (11 handlers), E-22/c10 (`admin restore`), resolved at Step 4
**Version**: 1.3.0
**Status**: draft (frozen when the producer task passes review)
**Last Updated**: 2026-11-14

## Purpose

Folder create, rename, move and delete, plus the path and device queries, as used by 11's node routes and 12's CLI. Signatures live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`); this file freezes the behaviour.

## Shape

Signatures: part 2 § 02 tree (`TreeService`). Behaviour per method:

| Method | Behaviour | Errors |
|---|---|---|
| `create_folder(account, parent, name)` | Name rules of helper 06; unique per folder by `name_key`; case-preserving; the folder copies the parent's vault membership (`vault_id`) read under the parent lock (v1.2) | `NameInvalid(reason)`, `NameConflict`, `ParentDeleted`, `NotFound` (missing or foreign parent), `NotAFolder` |
| `update_node(account, id, name?, parent?)` | Rename and move; no `base_revision`; case-only rename keeps the id; a move takes the account advisory lock and runs the cycle check (depth about 30); v1.2 (E-30/c01): a move is allowed only to a folder of the node's own vault membership or of the folder it leaves (inside one vault, or outside every vault; a vault root moves with its vault between folders outside every vault), else `VaultBoundary`; a move changes only parent and name, so a vault file's revisions, `client_meta` and blobs stay (AC-60) | adds `Cycle`, `VaultBoundary`, `IsRoot` |
| `delete_node(account, id, base_revision?)` | Files: stale base refused with the current revision; tombstoned node: success (idempotent); folders: one tombstone plus `cascade_pending`, descendants marked later | `EditedSince{current_revision}`, `IsRoot`, `NotFound` |
| `resolve_path(account, path)` | Node id of the live node at the root-relative path (`a/b.txt`, case-folded by `name_key`); every folder on the way must be live; `..`, empty, `.` segments, a leading or trailing `/` never resolve; a tombstoned last part is `NotFound` through the port. Inherent `PgTreeService::resolve_path_with_state` returns `(id, NodeState)`, the live node else the most recently tombstoned one (owner decision 2026-11-13, for restore) | `NotFound` |
| `find_revisions(account, client_device, page)` | Revisions whose `client_meta.device` matches (`None`: every revision of the account), offset paged in revision id order through the port's `Page{offset, limit}`; limit 0 gives `MAX_REVISION_PAGE` (1000), a larger limit is cut to it; reads `revisions_client_device_idx` | `InvalidCursor` (offset beyond `i64::MAX`) |

Stable codes (helper 02): `name_conflict` 409, `invalid_name` 422, `cycle` 409, `vault_boundary` 409 (v1.2), `edited_since` 409, `parent_deleted` 410, `parent_not_found` 404, `not_a_folder` 422, `invalid_cursor` 422, `not_found` 404.

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
- Resolved 2026-11-13 (owner): `resolve_path` finds a tombstoned last part for restore through `resolve_path_with_state`; several tombstones on one path: the most recent `deleted_at`, then `id DESC`. If a caller outside 02 needs the state, the port changes to `resolve_path -> (Uuid, NodeState)`.
- Resolved 2026-11-14 (owner, AZ-3715): `TreeRestore::restore_revision` (`PgTreeRestore`) resolves a path itself, every part being the live child of that name else its most recently tombstoned one (`deleted_at DESC, id DESC`), so a file under a deleted folder is reachable; `resolve_path` and `resolve_path_with_state` keep their live-folder rule. `FileMissing` and the reinstate refusal come from `BlobRegistry::reinstate_in_tx` (`ReinstateOutcome::FileMissing`, `ReinstateOutcome::BeyondRetention` for a removed `blobs` row); an `at` before `now - retention` is `BeyondRetention` without a transaction. Conflict-copy names of a restore carry no device label (`report (conflict copy YYYY-MM-DD HH-MM).txt`).
- Keyset paging for `find_revisions` (AZ-3716 AC-2) needs a `contracts` change (`after: Option<Uuid>`, `limit`, answer with `next`); until then the port pages by offset.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
| 1.1.0 | 2026-11-13 | `resolve_path` rules and `resolve_path_with_state`; `find_revisions` offset paging; `TreeService` implemented by `PgTreeService` | AZ-3716 |
| 1.2.0 | 2026-11-14 | Path rule and refusal sources of the operator restore (`PgTreeRestore`) | AZ-3715 |
| 1.3.0 | 2026-11-17 | `TreeRestore::preview_restore`: the restore transaction always rolled back (additive) | AZ-3796 |
| 1.3.0 | 2026-11-14 | v1.2 vault membership: `create_folder` copies the parent's `vault_id`; a move across the vault boundary is `VaultBoundary` (409 `vault_boundary`); `VaultRoots::claim_root` on `PgTreeService` (part 2) | AZ-3927 |
