# Gate S2: 100,000-file watcher, scan and memory per OS (spike)

**Task**: AZ-3881_gate_s2_100k_files_spike
**Name**: Gate S2 desktop budget spike
**Description**: Measure watcher setup, first scan, idle CPU and resident memory at 100,000 files on Linux and Windows, and record whether the AC-15 budget holds and what the daemon design must change if it does not
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3881
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c01

## Problem

AC-15 promises at most 1% idle CPU, at most 100 MB resident memory and a first scan within 60 s at 100,000 watched files. Only Linux numbers exist (index about 11 MB file, 7 MB resident). Windows CPU and memory at 100k files are unmeasured (14 §15 risk 1), and R27 says the 100 MB budget competes with in-flight upload buffers, hashing and SQLite. The watcher, scanner and index tasks need to know the budget holds before they are built.

## Outcome

- Measured numbers per OS (Linux C-REF, WIN-VM) for: watcher registration time and watch count, first scan (walk plus stat), resident memory with a 100,000-row index, idle CPU over 10 minutes, and memory with the in-flight upload cap of R27 (16 MiB) in use.
- A written verdict per OS (holds / does not hold) against the PT-14-1 targets and failure lines, recorded in 14 §15 risk 1, the R27 row of `risk_mitigations.md` and the E-24 risk table.
- If a target fails: the design change it forces (for example a lower in-flight cap or a different watcher mode), named against the task that must carry it (E-24/c06, E-24/c09 or E-23).
- Unblocks E-24/c06 and E-24/c09 and the PT-14-1 run of E-31.

## Scope

### Included
- A throwaway probe with the pinned crates (notify 8.2.1 recursive watcher, walk with no symlink following, rusqlite 0.41.1 WAL index of 100,000 rows) on a DS-11-shaped tree (100,000 files of 1 to 16 KiB, depth 6).
- Linux with `fs.inotify.max_user_watches` >= 524288 and once at the default limit (how the limit shows itself).
- Windows on WIN-VM (NTFS), standard user.

### Excluded
- The production watcher, scanner and index (E-24/c09, E-24/c06).
- macOS (v1.1, E-24/c23).
- Hashing CPU share (PT-14-2, E-24/c14).

## Acceptance Criteria

**AC-1: Numbers recorded per OS**
Given the probe on Linux C-REF and on WIN-VM with the DS-11-shaped tree
When it runs a cold start, a first scan and 10 minutes idle, three times per OS
Then the median and worst of each of the five measures (watch registration, first scan, resident memory, idle CPU, memory with the 16 MiB in-flight cap) are written in 14 §15 risk 1 with the date, the machine and the OS build

**AC-2: Verdict against AC-15**
Given the PT-14-1 targets (scan <= 30 s, idle <= 0.5% CPU, resident <= 80 MB) and failure lines (above 60 s, above 1%, above 100 MB)
When the numbers are compared
Then each OS has a pass or fail verdict per measure, and the R27 row of `risk_mitigations.md` shows the gate S2 result instead of "Open (gate S2)"

**AC-3: Watch-limit behaviour recorded**
Given Linux at the default `max_user_watches`
When the probe registers the 100,000-file tree
Then the spike records how the limit is reported to the daemon (error kind or event) and whether the watcher then misses changes silently, so E-24/c09 can detect it (NFT-RES-40)

**AC-4: Design consequences placed**
Given any failed measure
When the verdict is written
Then each consequence is a named change on an existing task (E-24/c06, E-24/c09, E-23 in-flight cap) or an owner question, and no work moves across epics

## Non-Functional Requirements

**Reliability**
- Absent WIN-VM hardware is recorded as `NOT-RUN(H-WIN)`, never as pass.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: measurement outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | DS-11 shape, Linux C-REF and WIN-VM | Probe run x3 per OS | Numbers and verdict recorded | PT-14-1, NFT-PERF-24, NFT-RES-LIM-06 |
| AC-3 | Default inotify limit | Probe registration | Limit signal recorded | NFT-RES-40 |

## Constraints

- The probe is throwaway and never becomes part of the daemon.
- Settings window excluded from all numbers (AC-15).

## Risks & Mitigation

**Risk 1: VM numbers differ from real hardware**
- *Risk*: WIN-VM is weaker or stronger evidence than a laptop.
- *Mitigation*: record the machine; the E-31 PT-14-1 run on the reference hardware stays the gate of record.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Reference Windows hardware is an owner item (perf gaps G1, G11); the spike uses WIN-VM | Owner | open | Low |
