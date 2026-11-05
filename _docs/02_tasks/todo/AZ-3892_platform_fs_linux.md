# PlatformFs trait and Linux implementation (openat2, freedesktop trash)

**Task**: AZ-3892_platform_fs_linux
**Name**: PlatformFs Linux
**Description**: The one file-system trait the daemon writes and reads through, with its Linux implementation: directory-handle writes that cannot leave the sync root, no symlink following, file ids, the case-insensitivity probe, remote deletes to the freedesktop trash, and the `FileSource` adapter sync-core hashes and uploads from
**Complexity**: 5 points
**Dependencies**: AZ-3767_sc_names, AZ-3967_sync_host_traits_foundation, AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3892
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_names/sc_names.md` — `FsProfile` and `portable_check` used to report `NameNotRepresentable` (E-05/c01).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `FileSource` (`len`, `read_at`, `identity`) (E-23/c14).

## Problem

AC-37 requires 0 writes outside the sync root and 0 overwritten unrelated files for a 40-case hostile corpus, including a directory swapped for a symlink between check and write. Path-string writes cannot guarantee that. Remote deletes must never unlink a file (R24): they go to the OS trash. Every other daemon part (watcher, applier, merger, uploads) must use one trait so tests can fake the OS and so Windows and macOS can plug in their own implementations.

## Outcome

- One `PlatformFs` trait with `open_root`, `is_case_insensitive`, `create_new_in`, `rename_over_in`, `trash`, `stat_no_follow`, `file_id`, `is_placeholder`, `open_for_read`, `walk`, and errors `Locked`, `Placeholder`, `EscapesRoot`, `NameNotRepresentable`, `TrashUnavailable`, `PermissionDenied`, `Io`.
- A Linux implementation where every open is relative to a directory handle with `openat2` and `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` (fallback `openat` with `O_NOFOLLOW` per component on kernels without `openat2`).
- Remote deletes moved into the freedesktop trash (data file plus `.trashinfo`), never unlinked.
- A `FileSource` adapter over an opened handle for sync-core, reporting `(size, mtime_ns)` identity.

## Scope

### Included
- The trait and its error type (OS-independent), and a fake implementation for other tasks' unit tests.
- Linux: `open_root`; `create_new_in` (temp file with the daemon's prefix, created exclusive); `rename_over_in` (atomic rename inside one directory handle); `trash` per the freedesktop trash specification (home trash, or the volume's `.Trash-<uid>` for another mount; `TrashUnavailable` when neither can be used); `stat_no_follow`; `file_id` for files and directories (device and inode); `open_for_read` with shared read; `walk` with no symlink following, reporting symlinks, sockets, FIFOs and devices as such.
- `is_case_insensitive` probe: create a lower-case random temp name in the root, open it by the upper-cased name, delete it; works on ext4 casefold folders.
- `is_placeholder` on Linux answers "no" (no placeholder system).
- `FileSource` adapter (`len`, `read_at`, `identity() -> (size, mtime_ns)`).

### Excluded
- Windows (E-24/c08) and macOS [v1.1] (E-24/c21) implementations.
- Deciding when to trash or write (E-24/c12b, E-24/c11); name escaping (E-24/c20).

## Acceptance Criteria

**AC-1: Writes cannot leave the root (AC-37)**
Given a root containing a subdirectory that a swapper thread replaces with a symlink to a directory outside the root, 10,000 times during writes
When the daemon creates and renames files in that subdirectory through `PlatformFs`
Then 0 files appear outside the root and each redirected write fails with `EscapesRoot` (ST-14-1)

**AC-2: Traversal and symlink names refused**
Given names `../x`, a path containing a symlink component, and an absolute path
When `create_new_in` or `open_for_read` is called with them
Then each returns `EscapesRoot` or `NameNotRepresentable` and nothing is created

**AC-3: No overwrite of an unrelated file**
Given an existing file `a.txt` in a directory
When `create_new_in` is asked for a temp file
Then the temp file has the daemon prefix and a new name, `a.txt` is unchanged, and `create_new_in` never truncates an existing file

**AC-4: Atomic replace**
Given a verified temp file and a target name
When `rename_over_in` runs
Then the target shows either the old or the new content at every moment (no partial file), and the rename stays inside the same directory handle

**AC-5: Remote delete to the trash (R24)**
Given a synced file in the home volume and one on a second mount
When `trash` is called for each
Then each appears in the freedesktop trash of its volume (data plus `.trashinfo` with the original path), 0 unlink calls are made, and with no usable trash the call returns `TrashUnavailable` and the file stays

**AC-6: File ids and walk**
Given files, directories, a symlink, a FIFO, a socket and a hard-link pair
When `walk` and `file_id` run
Then files and directories have stable ids across renames, the symlink, FIFO and socket are reported as their kinds without being followed or opened, and the two hard links are reported as two files

**AC-7: Case-insensitivity probe**
Given a normal ext4 root and an ext4 casefold folder
When `is_case_insensitive` runs
Then it answers false and true respectively and leaves no file behind

**AC-8: FileSource identity**
Given a file opened for upload
When it changes size or mtime while sync-core reads it
Then `identity()` reports the new `(size, mtime_ns)`, so sync-core's re-check before commit sees the change

## Non-Functional Requirements

**Compatibility**
- Kernels with and without `openat2` (fallback path tested on both).

**Reliability**
- No path string ever reaches a write system call; only names relative to a held directory handle.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | `../x`, symlink component, absolute path | `EscapesRoot` / `NameNotRepresentable`; nothing created |
| AC-3 | Temp creation next to an existing file | New prefixed name; existing file unchanged |
| AC-6 | Walk of mixed kinds | Kinds reported; nothing followed; hard links two files |
| AC-7 | Probe on ext4 and casefold | false / true; no leftover |
| AC-8 | Identity after a change | New size and mtime |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Swapper thread, 10,000 iterations, `openat2` and fallback | ST-14-1 (Linux) | 0 files outside the root; `EscapesRoot` | AC-37, NFT-SEC-31 |
| AC-4 | Reader polling during replace | Atomic replace | Old or new content only | AC-7 |
| AC-5 | Home volume and second mount; no trash | Trash calls | Trash entries; 0 unlinks; `TrashUnavailable` keeps the file | IT-14-3 (Linux trash half), FT-P-77 |

## Runtime Completeness

- Named capability: `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`, and the freedesktop trash (14 §2).
- Production code that must exist: real handle-relative system calls and a real trash move with `.trashinfo`.
- Allowed external stubs: the fake `PlatformFs` for other tasks' unit tests only.
- Unacceptable substitutes: path-string checks before a path-based write; unlink as a trash fallback.

## Constraints

- The trait is the only file-system access of the daemon outside the LocalIndex file.
- IT tier runs in the `desktop-linux` containers on ext4 and XFS.

## Risks & Mitigation

**Risk 1: Trash on removable or network volumes**
- *Risk*: no writable trash directory on that volume.
- *Mitigation*: `TrashUnavailable`; the caller marks and reports, nothing is unlinked.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | — | — | — |
