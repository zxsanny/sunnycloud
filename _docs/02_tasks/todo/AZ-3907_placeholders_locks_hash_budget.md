# Placeholders, skipped special files, lock retry and hashing budget

**Task**: AZ-3907_placeholders_locks_hash_budget
**Name**: Placeholders, locks, hashing budget
**Description**: Skip cloud placeholders before any open and count them; skip and count symlinks, junctions, sockets and FIFOs; refuse a pair root inside a cloud-managed folder; retry locked files with timer backoff and never report them deleted; and keep initial hashing at low priority within 25% of one core while the user is active
**Complexity**: 3 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3893_platform_fs_windows, AZ-3894_watcher_scanner_settler_rules, AZ-3961_sync_hash_file, AZ-3763_sc_config; notes: E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3907
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `hash_file` throttled by `hash_cpu_share` (E-23/c09a contribution).
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `SkippedEntry`, `SkipReason`, `CloudManagedRoot` (E-24/c05).

## Problem

Opening an evicted OneDrive file hydrates it, downloading gigabytes the user did not ask for; deleting it would propagate a loss (AC-32: 0 hydrated bytes, 100 skipped, 0 deletes). Office and antivirus lock files for seconds to minutes; a client that spins or treats the failed read as a delete wastes CPU or loses data (AC-33: at most 1% CPU, upload within 10 s after release, never reported deleted). Initial hashing of a big tree must not make the machine sluggish (AC-15: at most 25% of one core while the user is active, low priority).

## Outcome

- `is_placeholder` checked before any open; placeholder entries are `skipped_placeholder`, counted, listed by `list_skipped` with reason `Placeholder`, and never read, hydrated or deleted (also not deleted on the server when absent locally).
- Symlinks, junctions, sockets and FIFOs are skipped and counted with their reason; hard links are independent files.
- `add_pair` inside a detected cloud-managed folder is refused with `CloudManagedRoot` (check offered to E-24/c19).
- `Locked` reads retry with exponential backoff from `desktop.lock_retry_initial` (500 ms) to `desktop.lock_retry_max` (4 s) on timers, not loops; the entry is never `delete_held` or reported deleted because of a lock; antivirus stalls use the same path.
- Initial hashing runs at low process priority with `desktop.hash_cpu_share` (0.25) passed to sync-core's throttled `hash_file`; the idle loop has no periodic work faster than the rescan timer.
- The `desktop.lock_retry_initial`, `desktop.lock_retry_max` and `desktop.hash_cpu_share` rows added to helper 01's key table.

## Scope

### Included
- Skip policy and counting, `list_skipped` data, cloud-managed root detection, lock backoff, hashing priority and share.

### Excluded
- Attribute detection per OS (E-24/c07, E-24/c08; macOS [v1.1] E-24/c21); the hashing loop and its throttle (E-23/c09a); `DaemonStatus` assembly (E-24/c19).

## Acceptance Criteria

**AC-1: Placeholders never opened (AC-32)**
Given a fake `PlatformFs` where 100 of 500 files are placeholders
When the pair scans and syncs
Then 100 entries are `skipped_placeholder`, `list_skipped` returns them with reason `Placeholder`, `open_for_read` is called 0 times on them, and 0 delete requests are sent for them

**AC-2: Real placeholders on Windows**
Given DS-14 on WIN-VM (400 hydrated, 100 dehydrated with the cfapi test provider)
When the daemon syncs the tree
Then the provider's callback log shows 0 hydrated bytes, 100 files are listed as skipped, and 0 deletes are propagated (IT-14-9)

**AC-3: Cloud-managed root refused**
Given a local root inside a detected cloud-managed folder
When `add_pair` checks it
Then it fails with `CloudManagedRoot`

**AC-4: Special files skipped**
Given a symlink, a junction (Windows), a socket and a FIFO in a pair, and a hard-link pair
When the pair scans
Then the four are listed by `list_skipped` with their reason and never followed or read, and the hard links sync as two independent files

**AC-5: Lock backoff without spin (AC-33)**
Given `open_for_read` returning `Locked` 5 times then succeeding, with the config mock at 50 ms initial and 200 ms maximum
When the pair worker retries
Then the gaps are 50, 100, 200, 200, 200 ms (plus or minus 20%) on `ManualClock` timers and 0 `delete_node` calls are made (IT-14-10 unit)

**AC-6: Locked file on Windows**
Given DS-28 holding an exclusive lock for 60 s on WIN-VM
When the daemon waits for it
Then daemon CPU stays at or below 1%, the file uploads within 10 s after the release, and the entry is never `delete_held` (IT-14-10, PT-14-3)

**AC-7: Hashing budget (AC-15)**
Given 20 GiB of small files to hash while a UI-activity simulator runs
When initial hashing runs
Then daemon thread CPU is at or below 25% of one core (failure above 30%) and the process runs at low priority (PT-14-2)

## Non-Functional Requirements

**Performance**
- Lock retries bounded by `desktop.lock_retry_max`; no busy loop anywhere.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Fake placeholders | Skipped, listed, 0 opens, 0 deletes |
| AC-3 | Root inside a fake cloud-managed folder | `CloudManagedRoot` |
| AC-4 | Fake walk with special kinds | Listed with reason; not followed |
| AC-5 | `Locked` x5 then success on `ManualClock` | 50, 100, 200, 200, 200 ms; 0 deletes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | DS-14, cfapi test provider, WIN-VM | IT-14-9 | 0 hydrated bytes; 100 skipped; 0 deletes | AC-32, FT-P-55, NFT-RES-17 (H-WIN) |
| AC-6 | DS-28 60 s lock, WIN-VM | IT-14-10, PT-14-3 | CPU <= 1%; upload <= 10 s after release | AC-33, FT-P-56, NFT-RES-15, NFT-RES-LIM-07 (H-WIN) |
| AC-7 | 20 GiB small files; UI-activity simulator | PT-14-2 | <= 25% of one core; low priority | AC-15 |

## Constraints

- Linux locks are advisory: IT-14-10's Linux run is "not applicable", never pass.
- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- WARN when a placeholder is skipped; no user paths above DEBUG (14 §9).

## Risks & Mitigation

**Risk 1: Cloud-managed folder detection misses a provider**
- *Risk*: a root inside an unknown provider's folder is accepted.
- *Mitigation*: placeholders inside it are still skipped by attribute (AC-1, AC-2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How the daemon detects "user active" for the 25% share is not specified (PT-14-2 uses a UI-activity simulator) | E-24 | open | Medium |
| How a "cloud-managed folder" is detected (which providers, which markers) is not specified | E-24 | open | Low |
| The data model has no stored state for skipped symlinks and special files (see E-24/c06) | E-24 | open | Low |
