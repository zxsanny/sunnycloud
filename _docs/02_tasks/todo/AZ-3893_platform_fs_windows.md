# PlatformFs Windows implementation (handle-relative create, recycle bin, placeholders)

**Task**: AZ-3893_platform_fs_windows
**Name**: PlatformFs Windows
**Description**: The Windows implementation of `PlatformFs`: handle-relative creates and renames with reparse-point checks so junctions and symlinks cannot redirect writes, remote deletes to the recycle bin, Cloud Files placeholder detection without hydration, shared-read opens that report locks, and the case-insensitivity probe on NTFS
**Complexity**: 5 points
**Dependencies**: AZ-3892_platform_fs_linux, AZ-3767_sc_names, AZ-3652_initial_structure; notes: E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3893
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_names/sc_names.md` — `FsProfile` for NTFS (reserved names, trailing dot or space, ADS syntax) (E-05/c01).

## Problem

Windows is a v1.0 platform. Its hostile cases differ from Linux: junctions, alternate data streams, reserved device names, trailing dots and spaces, and OneDrive placeholders that hydrate when opened (AC-32: 0 bytes of hydration for 100 evicted files). Remote deletes must go to the recycle bin, and exclusive locks (Office, antivirus) must show as `Locked` so they are retried, not reported deleted (AC-33). The trait from E-24/c07 needs a Windows implementation with the same guarantees.

## Outcome

- `PlatformFs` for Windows: every create, open and rename relative to a held directory handle (`NtCreateFile` with a root directory handle), reparse points checked so a junction or symlink component yields `EscapesRoot`.
- `trash` through the shell recycle-bin API; `TrashUnavailable` when the volume has none; never a delete.
- `is_placeholder` from file attributes (Cloud Files, OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS) read without opening the file's data.
- `open_for_read` with shared read and write that maps a sharing violation to `Locked`.
- `file_id` for files and directories (volume serial plus file id), `is_case_insensitive` probe on NTFS.

## Scope

### Included
- The trait methods of E-24/c07 on NTFS for a standard user.
- Names the platform cannot hold (reserved device names, trailing dot or space, `:` stream syntax, over-length) reported as `NameNotRepresentable`, so the caller escapes them (E-24/c20).
- Detection of the junction, symlink and other reparse kinds in `walk`, without following them.

### Excluded
- Linux (E-24/c07), macOS [v1.1] (E-24/c21).
- What the daemon does with a placeholder or a lock (E-24/c14); escaping (E-24/c20); the cfapi test provider (E-31/c22).

## Acceptance Criteria

**AC-1: Junctions and symlinks cannot redirect writes (AC-37)**
Given a root on NTFS in which a swapper replaces a subdirectory with a junction to a folder outside the root, 10,000 times during writes
When the daemon creates and renames files there through `PlatformFs`
Then 0 files appear outside the root and the redirected operations fail with `EscapesRoot` (ST-14-1 Windows half)

**AC-2: Unrepresentable names reported**
Given names `CON.txt`, `a.`, `a `, `a:stream` and a name over the path limit
When `create_new_in` is asked for them
Then each returns `NameNotRepresentable` and nothing is created (no ADS written, no device opened)

**AC-3: Remote delete to the recycle bin (R24)**
Given a synced file
When `trash` is called
Then the file is in the user's recycle bin with its original path, 0 delete calls were made, and on a volume with no recycle bin the call returns `TrashUnavailable` and the file stays

**AC-4: Placeholders detected without hydration (AC-32)**
Given DS-14 (400 hydrated, 100 dehydrated OneDrive-style files from the cfapi test provider)
When `walk` and `is_placeholder` run over the tree
Then exactly the 100 dehydrated files are placeholders and the provider's callback log shows 0 hydrated bytes

**AC-5: Locks reported, not hidden (AC-33)**
Given a file held with an exclusive lock (DS-28)
When `open_for_read` is called
Then it returns `Locked`, not `Io` or not-found

**AC-6: File ids, walk and probe on NTFS**
Given NTFS files, directories, a junction, a symlink and a hard-link pair
When `walk`, `file_id` and `is_case_insensitive` run
Then ids are stable across renames, the junction and symlink are reported as their kinds without being followed, hard links are two files, and the probe answers true

## Non-Functional Requirements

**Compatibility**
- Windows 10 and 11, standard user, NTFS; results on WIN-VM, absent hardware `NOT-RUN(H-WIN)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Reserved, trailing dot, trailing space, stream and long names | `NameNotRepresentable`; nothing created |
| AC-6 | Attribute and reparse-tag classification of walk results (recorded attribute sets) | Correct kinds; nothing followed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WIN-VM, junction swapper x10,000 | ST-14-1 (Windows) | 0 files outside the root | AC-37, NFT-SEC-31 |
| AC-3 | WIN-VM, synced file; volume without recycle bin | Trash | In recycle bin; 0 deletes; `TrashUnavailable` keeps the file | IT-14-3 (Windows half, H-WIN) |
| AC-4 | DS-14 with cfapi test provider | Placeholder detection | 100 placeholders; 0 hydrated bytes | IT-14-9 (H-WIN), NFT-RES-17 |
| AC-5 | DS-28 exclusive lock | Open for read | `Locked` | IT-14-10 (H-WIN) |
| AC-6 | WIN-VM NTFS tree | Ids, walk, probe | Stable ids; kinds; probe true | IT-14-7 (NTFS) |

## Runtime Completeness

- Named capability: handle-relative `NtCreateFile` with reparse-point checks, the shell recycle-bin API, Cloud Files placeholder attributes (14 §2, §5 step 9).
- Production code that must exist: real Windows calls on NTFS.
- Allowed external stubs: the cfapi test provider (DS-14) as the source of placeholders in tests.
- Unacceptable substitutes: path-string checks before a path-based write; `DeleteFile` as a trash fallback; opening a file to test whether it is a placeholder.

## Constraints

- Runs as a standard user; no elevation, no service.
- Lab-only cases run on WIN-VM through the lab controller (E-31/c16).

## Risks & Mitigation

**Risk 1: Attribute sets differ across OneDrive versions**
- *Risk*: a placeholder is missed and opened, hydrating it.
- *Mitigation*: check every documented attribute; the DS-14 provider covers the dehydrated state; NFT-RES-17 runs with real OneDrive in E-31.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Windows reserved-name escape marker (`%5F`) and ` (2)` suffix are open owner items; this task only reports `NameNotRepresentable` | Owner | open | Low |
