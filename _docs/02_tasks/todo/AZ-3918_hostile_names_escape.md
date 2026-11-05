# Hostile and unrepresentable names: %XX escape, list_escaped, rejected names, corpus run

**Task**: AZ-3918_hostile_names_escape
**Name**: Hostile names
**Description**: Save server names the local file system cannot hold under a `%XX`-escaped name that never overwrites another file, mark and list them with the original name, mark and report a local name the server rejects without retrying it in a loop, and run the 40-case hostile corpus per OS
**Complexity**: 3 points
**Dependencies**: AZ-3891_local_index_store, AZ-3901_decide_from_disk_pair_worker, AZ-3905_change_applier, AZ-3767_sc_names; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3918
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c20

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_names/sc_names.md` — `portable_check`, `escape_name`, `FsProfile`, `Reason` (E-05/c01).
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `EscapedEntry` (E-24/c05).

## Problem

The server accepts names a local file system cannot store (`a:b` on Windows, reserved device names, bidi characters, names that collide by case) and a hostile server could send traversal names. AC-37: a 40-case corpus per OS causes 0 writes outside the root and 0 overwritten files. The other way round, a local file whose name the server rejects (for example a bidi-control character) must not be retried forever and must not stop the rest of the pair (FT-N-51).

## Outcome

- On apply (in the write path of E-24/c12b), each server name passes `portable_check` for the root's `FsProfile`; a name that needs escaping is saved under `escape_name`'s `%XX` form, which is collision-free against the folder's current names, so an existing file is never overwritten; the entry is `name_escaped` with `escaped_from` holding the original name; WARN "name escaped".
- An escaped name is local only: it is never uploaded back as a rename; the server keeps the original.
- `list_escaped` returns `{original_name, saved_name, rel_path}`; `escaped_count` in status.
- A local file whose upload ends with the server's `InvalidName` (from the pair worker of E-24/c10a) is marked and reported with the server's reason; it is not retried while the file is unchanged; a rename or edit makes it a new candidate; the rest of the pair keeps syncing.
- The DS-12 corpus run per OS (Linux, Windows) proving 0 writes outside the root and 0 overwrites.

## Scope

### Included
- Escape on apply, its index state and listing; the rejected-name state and its retry rule; the corpus run.

### Excluded
- The escape rules themselves (E-05/c01); handle-relative writes (E-24/c07, E-24/c08); server-side name validation (E-13).

## Acceptance Criteria

**AC-1: Corpus causes no damage (AC-37)**
Given DS-12 for the running OS created on the server through the API
When the daemon applies it
Then 0 files exist outside the root (tree diff of the parent), sentinel file hashes are unchanged, unrepresentable names are saved `%XX`-escaped in state `name_escaped`, and symlinks, junctions, sockets and FIFOs appear in `list_skipped` (IT-14-14)

**AC-2: Escaped names listed**
Given server names `Q3: plan?.docx` and `notes <draft>.md` applied on Windows
When `list_escaped` is called
Then it returns each with `original_name`, `saved_name` (`Q3%3A plan%3F.docx`, `notes %3Cdraft%3E.md`) and `rel_path`, and `status.escaped_count` is 2

**AC-3: Escape never overwrites (ST-14-5)**
Given the server holds `a:b` and a real file named `a%3Ab`
When both are synced to a Linux and a Windows root
Then both files survive and the escaped one is in `list_escaped`

**AC-4: Escaped names are not uploaded back**
Given an escaped local file whose content the user then edits
When the pair worker uploads the edit
Then it updates the original server node and never renames it to the escaped name

**AC-5: Server-rejected name reported, no retry loop (FT-N-51)**
Given a pair with 10 ordinary files and `a<U+202E>b.txt` (both OS) and `a<U+0007>b.txt` (Linux only)
When the files are created, then the proxy counts upload attempts for 10 minutes, then `a<U+202E>b.txt` is renamed to `ab.txt`
Then the 10 ordinary files are on the server within 5 s after settle, each rejected name is reported with the server's reason while the pair stays active, there are 0 further attempts after the first 4xx while the file is unchanged, and `ab.txt` uploads within 5 s and leaves the report (11 files uploaded; rejected 2 of 2 on Linux, 1 of 1 on Windows)

## Non-Functional Requirements

**Security**
- No name from the server is ever joined into a path string for a write; names go to `PlatformFs` as single components.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Escape on apply with a fake `PlatformFs` | Saved name, state, `escaped_from` |
| AC-3 | Escape when the escaped form already exists | Collision-free name; existing file untouched |
| AC-4 | Edit of an escaped file | Upload to the original node; no rename |
| AC-5 | `InvalidName` from a fake `SyncApi`; unchanged file; then rename | Reported once; 0 retries; renamed file uploaded |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-12 per OS (reserved names, ADS, junctions on WIN-VM) | IT-14-14 | 0 outside root; 0 overwrites; escaped and skipped listed | AC-37, FT-N-19, NFT-SEC-31 (H-WIN) |
| AC-3 | `a:b` and real `a%3Ab` | ST-14-5 | Both survive (Windows reserved-name marker and ` (2)` suffix BLOCKED) | AC-37 |
| AC-5 | `desktop-linux-a` and WIN-VM; DS-12 tricky names; proxy log | FT-N-51 | 11 uploaded; rejected reported 2/2 Linux, 1/1 Windows; 0 retries | AC-37 |

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); FT-N-51 as a system scenario needs the whole daemon and runs once E-24/p02 composes it.
- WARN "name escaped" without the user's path above DEBUG (14 §9).

## Risks & Mitigation

**Risk 1: Escape defaults change**
- *Risk*: the owner changes the reserved-name marker or the suffix.
- *Mitigation*: they are named constants in `sc_names` (minor change; escaped names are local only).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Windows reserved-name marker (`%5F`) and the ` (2)` suffix are open owner items; ST-14-5's Windows half is BLOCKED | Owner | open | Medium |
| Where a rejected name is "reported with the server's reason" (no `DaemonStatus` field or list method; mockup "Errors") | E-24 with owner | open | Medium |
| No entry state or column stores a rejected name (see E-24/c06) | E-24 | open | Low |
| FT-N-51 and NFT-RES-40 are not in any E-31 row; they are placed on E-24/c20 and E-24/c09 | Parent | open | Low |
