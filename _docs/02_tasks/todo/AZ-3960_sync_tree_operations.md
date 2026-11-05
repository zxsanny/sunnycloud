# Sync core tree operations: listing, revisions, lookups, create, move, delete

**Task**: AZ-3960_sync_tree_operations
**Name**: Tree operations
**Description**: The `SyncApi` tree calls: `root_node`, `list_children` with `content_hash`, `get_revision`, chunked `lookup_revisions`, `create_folder`, `move_node` without a base revision and `delete_node` with one
**Complexity**: 3 points
**Dependencies**: AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3960
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — tree operations (this task contributes them).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `NodeView`, `ListingPage`, `RevisionView`, `RevisionRef`, `LOOKUP_CHUNK`.
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — tree routes (draft).
- `_docs/02_document/contracts/02_tree/tree_reader.md` — server listing and lookup semantics.

## Problem

The desktop first-sync merge needs file listings with `content_hash` to upload 0 identical bytes (AC-31). The mobile reinstall rebuild needs `lookup_revisions` in chunks of 1,000 (AC-46). Deletes must carry the base revision so an edit on another device wins (AC-8), while a move must never carry one, so a move never conflicts with an edit (AC-70). Server-supplied names must stay data (AC-37). None of these calls exist on the client yet.

## Outcome

- The seven tree operations on `HttpSyncClient`, typed as in the `sync_api` contract.
- `lookup_revisions` splits any input into requests of at most 1,000 hashes and merges the results.
- Request bodies proven against the scripted fake server: `delete_node` of a file carries `base_revision`, `move_node` never does.

## Scope

### Included
- `root_node`, `list_children` (page token chaining to `next = None`), `get_revision`.
- `lookup_revisions` chunking at 1,000 and result merge; unknown hashes return nothing.
- `create_folder`, `move_node` (by node id, no base revision), `delete_node` (base revision for files only) and their typed errors.
- Names returned verbatim, never interpreted as paths.

### Excluded
- Uploads, including the conflict copy on upload and the new-file name collision (E-23/c09b, c10).
- The real-server legs of IT-13-6 and IT-13-14 (E-23/c18a).
- Path safety on the host (14, 06).

## Acceptance Criteria

**AC-1: Listings carry content hashes (AC-31)**
Given a scripted listing of 2,500 entries over several pages, files and folders mixed
When `list_children` is called page after page
Then every file item carries its `content_hash`, folders carry none, and the pages chain until `next = None`

**AC-2: Lookups chunked at 1,000 (AC-46)**
Given 2,500 known and 10 unknown hashes
When `lookup_revisions` is called once
Then the fake server sees exactly 3 requests of 1,000, 1,000 and 510 hashes, and the result holds 2,500 `RevisionRef` with their `client_meta`

**AC-3: Delete carries the base revision (AC-8)**
Given a file whose server revision moved from rev1 to rev2
When `delete_node(file, rev1)` is called and the server answers `edited_since`
Then the request carries `base_revision = rev1` and the result is `EditedSince{current}`

**AC-4: Move carries no base revision (AC-70)**
Given a folder that contains a file being edited elsewhere
When `move_node(folder, new_parent, name)` is called
Then the request body has no `base_revision` and the node id is unchanged in the result

**AC-5: Typed tree errors**
Given scripted answers `name_conflict`, `invalid_name`, `cycle`, `parent_not_found`, `parent_deleted` and `not_a_folder`
When `create_folder` or `move_node` meets each
Then the results are `NameConflict`, `InvalidName`, `Cycle`, `ParentNotFound`, `ParentGone` and `NotAFolder`

**AC-6: Hostile names stay data (AC-37)**
Given listing names `../../etc/passwd`, a name with U+202E and an 8 KiB name
When `list_children` returns them
Then the names are returned byte for byte and no file system call is made with them

## Non-Functional Requirements

**Performance**
- Lookup chunks are sent on the shared HTTP/2 connection; no request above 1,000 hashes.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Chunk split and merge for 0, 1, 1,000, 1,001 and 2,510 hashes | Requests of at most 1,000; merged result |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake server, 2,500 entries | Paged listing | Hashes on files only; pages chain | IT-13-14, FT-P-54 |
| AC-2 | Fake with 2,500 known hashes | Lookup | 3 requests; 2,500 results | IT-13-14, FT-P-32 |
| AC-3 | Fake answering `edited_since` | Delete with rev1 | `base_revision` sent; `EditedSince` | IT-13-6, FT-P-10 |
| AC-4 | Fake with body log | Move | No `base_revision`; same id | IT-13-6, FT-N-42 |
| AC-5 | Fake answering each code | Create and move | Typed error per code | IT-13-13 |
| AC-6 | Fake with hostile names | Listing | Verbatim names | IT-13-19 |

## Constraints

- Server order decides conflicts (AC-8); the client never renames on its own (ADR-005).
- `lookup_revisions` is used only for the reinstall rebuild, never to skip upload bytes (no cross-node dedupe, `canonical-names.md`).

## Risks & Mitigation

**Risk 1: `revisions:lookup` body class**
- *Risk*: 1,000 hashes as hex JSON (about 70 KB) exceed the `Small` 64 KiB body class.
- *Mitigation*: flagged to E-21 (`api_v1_surface.md` open point 4); the client keeps the 1,000 chunk size from 13.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `revisions:lookup` body class unassigned; 1,000 hashes exceed `Small` (`api_v1_surface.md` open point 4) | E-21 owner | open | Low |
| `EditedSince{current}` needs the current revision id, which the problem body does not carry (`sync_api.md` open point 2) | E-02 / E-21 | open | Medium |
