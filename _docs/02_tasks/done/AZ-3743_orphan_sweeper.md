# OrphanSweeper with modes and abort on partial scans

**Task**: AZ-3743_orphan_sweeper
**Name**: Orphan sweeper
**Description**: The janitor that finds blob files without a row older than `orphan_sweep_age`, logs or deletes them by mode, and deletes nothing when any part of a scan fails
**Complexity**: 3 points
**Dependencies**: AZ-3724_blob_file_format, AZ-3729_blob_registry_gc_queue, AZ-3765_sc_clock, AZ-3768_contracts_part1
**Component**: 04 blobstore
**Tracker**: AZ-3743
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c11

## Problem

`write_complete` writes the file before its caller inserts the row, so a crash in between leaves a file nobody references (AC-68). Such orphans must go after a safe age (48 h). After a database restore, rows can be missing for files that are still live; a sweep that trusts a partial listing would delete live data, so a scan that is incomplete must delete nothing.

## Outcome

- `OrphanSweeper::run_cycle(now)` (a `Janitor`) walks the 65,536 prefix directories; per directory it lists the ids on disk and queries `blobs` for the matching id range; a file without a row whose mtime is older than `orphan_sweep_age` is an orphan.
- Modes from `orphan_mode`: `off` (nothing), `report` (WARN per orphan, nothing deleted), `delete` (logged, then deleted after a re-check that the row still does not exist).
- Any listing or query failure, or a partial listing, aborts the whole run with 0 deletions and an ERROR log.

## Scope

### Included
- The walk, the age rule, the three modes, the re-check before delete, abort-on-partial, INFO cycle summary.

### Excluded
- Scheduling (E-22/c06, every `orphan_sweep_interval`), the restore advice to use `report` first (E-22 restore procedure), session-reaper file removal (03 calls `discard`).

## Acceptance Criteria

**AC-1: Delete mode removes only old orphans**
Given `orphan_sweep_age = 500 ms`, 100 files with rows, 20 orphans older than the age and 10 younger
When a cycle runs in `delete` mode
Then the 20 old orphans are removed and the 10 young ones and all 100 files with rows remain

**AC-2: Report and off modes delete nothing**
Given the same files
When a cycle runs in `report` mode and then in `off` mode
Then `report` deletes 0 files and logs 20 WARN lines (ids only), and `off` does nothing

**AC-3: Partial scans abort**
Given an unreadable prefix directory in one run and an injected database error mid-run in another
When a cycle runs in `delete` mode
Then the whole run aborts with 0 deletions and an ERROR log line

**AC-4: A racing writer keeps its file**
Given an orphan candidate whose row is inserted by a writer after the listing but before the delete
When the sweeper reaches it
Then the file is kept

**AC-68: An interrupted write leaves no permanent orphan (component part, R1)**
Given a synced file from `write_complete` whose caller crashed before inserting its row, and an `uploading` blob whose row exists
When the orphan sweep runs in `delete` mode with the clock 1 ms before and then past `orphan_sweep_age` (48 h default)
Then nothing is removed before the age, after it only the rowless file is removed, and the `uploading` blob's file stays (its row is removed by 03's session reaper, which also releases its quota); the crash between blob write and commit through the real server is checked by E-22/p03

## Non-Functional Requirements

**Performance**
- One walk per `orphan_sweep_interval` (24 h default); no cursor state is kept between runs.

**Reliability**
- 0 deletions from any incomplete scan.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Age rule at the boundary | Older than age only |
| AC-3 | Abort flag after a partial listing | No delete step runs |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100 rowed, 20 old, 10 young orphans | IT-04-14 delete mode | 20 removed | |
| AC-2 | Same files | IT-04-14 report and off modes | 0 removed; 20 WARN lines | |
| AC-3 | Unreadable directory; DB error | IT-04-14 abort cases | 0 deletions; ERROR | |
| AC-4 | Row inserted after listing | IT-04-14 race case | File kept | |
| AC-68 | Rowless synced file; `uploading` blob | Age boundary with the manual clock | Only the rowless file removed after the age | NFT-RES-34, NFT-RES-04, FT-N-40 |

## Constraints

- Runs in system scope; holds no transaction across a directory listing or an unlink.
- Files are matched to rows by blob id; the sweep never builds a path from anything but a blob id.

## Risks & Mitigation

**Risk 1: Orphan sweep after a restore deletes live data**
- *Risk*: rows are missing because the database is older than the volume.
- *Mitigation*: abort on partial scans; `report` first after a restore (E-22 procedure, AT-04-3 in E-10/c13).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The walk is described as rate-limited, but no rate setting exists in the helper config table | Epic owner | open | Low |
| What happens to a file in a prefix directory whose name is not a blob id is not stated (the description only covers "a file with no row") | Epic owner | open | Low |
