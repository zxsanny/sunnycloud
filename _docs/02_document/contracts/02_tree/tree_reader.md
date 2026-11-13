# Contract: tree-reader (TreeReader and LiveUsageSource behaviour)

**Component**: 02_tree
**Producer task**: E-13/c03 — `_docs/02_tasks/todo/B03-03_tree_reader.md` (contributors: E-13/c12 `B03-12_tree_revision_lookups.md`, E-13/c14 `B03-14_tree_thumb_state_listing.md`); keys assigned at Step 4
**Consumer tasks**: E-14/c02, E-14/c04, E-14/c08, E-14/c10 (this batch); E-16 (06 sharing), E-17 (07 admin), E-18 (08 media_jobs), E-21 (11 http_api), E-22 (12 operator_cli), E-30 (20 vault_service), resolved at Step 4
**Version**: 1.2.0
**Status**: draft (frozen when the producer task passes review)
**Last Updated**: 2026-11-04

## Purpose

The read side of the tree, used by seven components that never touch `nodes` or `revisions`. Rust signatures and DTO fields live in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`, E-00/c04); this file freezes the behaviour consumers may rely on.

## Shape

Signatures: part 2 § 02 tree (`TreeReader`, `LiveUsageSource`). Behaviour per method:

| Method | Behaviour | Errors |
|---|---|---|
| `get_node(account, Id \| Root)` | `NodeView` of a node of this account; a node under a tombstoned ancestor is reported as deleted (bounded parent-chain walk, independent of the cascade) | `NotFound` (also another account's id) |
| `list_children(account, folder, after_name_key?, limit?)` | Live children ordered by `name_key`; keyset cursor; default `listing_default_limit` (1000), clamped to `listing_max_limit`; items carry size, client mtime, content hash, media type, `thumb_state` (`ready`, `pending`, `failed`, `none`; `none` when no `ThumbnailStateSource` is wired) | `NotFound`, `NotAFolder`, `InvalidCursor` |
| `find_live_node(account, parent, name)` | The live child whose `name_key` equals the folded `name`; tombstones never match | none (absent result) |
| `resolve_live_file(account, node)` | `Found(LiveFile{kind = File, revision = current, blob_ref = blob id})`, `Found(LiveFile{kind = Folder})` with empty revision and blob ref, or `Gone` (tombstoned, purged, other account, or any ancestor tombstoned) | none |
| `root_node(account)` | The account's single root | none |
| `get_revision(account, revision)` | `RevisionView` including the blocklist | `NotFound` |
| `is_current_or_retained(account, revision)` | true while the revision is current or retained (not yet collected); false for collected or foreign revisions | none |
| `describe_blobs(blob_ids)` | System scope (doctor, AC-73): for each blob id the paths of the nodes that use it; unknown ids map to nothing | none |
| `lookup_revisions(account, hashes)` | At most 1000 hashes; current revisions of live files only, with `client_meta` | `BatchTooLarge` above 1000 |
| `LiveUsageSource::live_bytes(account)` | Sum of the sizes of current revisions of live files | none |

## Invariants

- Every call is scoped by account; a foreign id gives exactly the answer of a random id (`NotFound`, `Gone`, absent, empty) (AC-59).
- "Is a folder" and "gone" are distinct results; a file under a tombstoned ancestor is gone on the next call, without waiting for the 30 s cascade (AC-12, AC-77).
- A node id is stable across rename, move and save-in-place (AC-34, AC-77).
- No result is cached: a delete is visible on the next call.
- A listing is an index scan on `nodes_parent_name_key` plus primary-key lookups; at most one `ThumbnailStateSource` call per page, with image and video hashes only.
- `lookup_revisions` never matches replaced or deleted revisions; there is no commit-by-hash.
- Users are never served non-current revisions (03's rule); `is_current_or_retained` exists for 08's lease check only.

## Non-Goals

- Not covered: writes (`tree_service.md`, `tree_commit.md`); path resolution (`TreeService::resolve_path`); restore.
- Not covered: HTTP routes and status mapping (11).

## Versioning Rules

- **Breaking changes** (a result variant removed, a scoping rule relaxed, the folder/gone split merged) require a new major version and a `contracts` change task in E-13.
- **Non-breaking additions** (a new optional `NodeView` or listing field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| listing-complete | DS-02, default limit, follow `next` | 10 pages, 10,000 unique names in `name_key` order, no `next` at the end | IT-02-16 |
| folder-vs-gone | live folder; tombstoned file; file under tombstoned ancestor before cascade | `Found(Folder)`; `Gone`; `Gone` | IT-02-17 |
| foreign-id | account B with A's node id | same result as a random id | ST-02-1 |
| lookup-limit | 1001 hashes | `BatchTooLarge` | IT-02-18 |
| live-bytes | created, replaced and deleted files | equals the live current sum | IT-02-15 |

## Open items

- Implemented reads (AZ-3707, 2026-11-13): `get_node`, `list_children`, `find_live_node`, `resolve_live_file`, `root_node` on `PgTreeReader`; the revision reads and `impl TreeReader for PgTreeReader` are implemented (AZ-3716). `NodeView` has no `thumb_state` field yet; it needs a `contracts` change with AZ-3718. `root_node` answers `NotFound` only for an erased account.

- `describe_blobs` (proposed default 2026-11-13, AZ-3716): system scope, one statement for the list; a blob of a file whose node and ancestors are live maps to `[path]` (root-relative, `/`-joined); a blob of a tombstoned file or of a file under a tombstoned folder maps to `[]` (deleted, no path); an id no revision holds (random, derived) is absent.
- `get_revision` answers any revision row of the account (current or retained) with its blocklist; `is_current_or_retained` is true while the revision row exists. `lookup_revisions` takes at most `MAX_LOOKUP_HASHES` (1000), checked before any query, one statement on `revisions_account_id_content_hash_idx`. `lookup_revisions` and `LiveUsageSource::live_bytes` (on `PgTreeReader`) count a file as live when its own node is not tombstoned, so a file under a tombstoned folder counts until the cascade marks it.
- `listing_max_limit` defaults to 1000 (owner decision 2026-11-13): a larger requested page is cut to it.
- Part 2 open point 6: `NodeView`, `RevisionView`, `ListingPage` fields are derived from `data_model.md`; additions are minor.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B03) |
| 1.1.0 | 2026-11-13 | Provider `PgTreeReader::new(db, &TreeConfig)` with inherent reads (the `TreeReader` trait impl lands with AZ-3716, which adds the revision reads); `limit = 0` is the default; a cursor that cannot be a stored `name_key` is `InvalidCursor`; listing a folder under a tombstoned ancestor is `NotFound`; `thumb_state` is not on `NodeView` yet (contracts change with AZ-3718) | AZ-3707 |
| 1.2.0 | 2026-11-13 | Revision reads, `describe_blobs` default, `TreeReader` and `LiveUsageSource` implemented | AZ-3716 |
