# Watcher, Scanner, Settler and RuleMatcher

**Task**: AZ-3894_watcher_scanner_settler_rules
**Name**: Watcher, scanner, settler, rules
**Description**: The local-change front end of each pair: OS watcher hints (paths only), safety rescans at start, on a timer, after wake and on watcher error, gitignore-style include and exclude rules applied first, and a settler that releases a path only after its size and mtime stayed unchanged for 2 s; with a rescan-only fallback when the watch limit is hit, reported in status
**Complexity**: 5 points
**Dependencies**: AZ-3881_gate_s2_100k_files_spike, AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3763_sc_config, AZ-3765_sc_clock, AZ-3652_initial_structure; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9); E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3894
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_config/sc_config.md` — key table rows and durations (E-01/c01).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `ManualClock`, `wake_gap_exceeded` (E-03/c01).

## Problem

OS watchers lose events: notify 8.2.1 does not signal a Windows buffer overflow, inotify has a per-user watch limit, network shares send nothing, and a suspended laptop misses everything. The daemon must therefore treat events as hints and find changes by rescanning, and it must not upload a file while it is still being written (AC-18: only after 2 s unchanged). Excluded paths must never be uploaded, downloaded or deleted (AC-66). After a 30-minute suspend, devices must converge within 60 s (AC-35).

## Outcome

- Per pair: a recursive watcher that turns any event into "this path may be dirty", with no trust in the event kind.
- A scanner (walk plus stat, no symlink following) that diffs disk against the index and emits the same hints; it runs at start, every `desktop.rescan_interval` (15 min), after a clock gap above `desktop.wake_gap` (60 s) and on watcher error.
- A rule matcher with the `ignore` crate's gitignore grammar (`*`, `**`, `?`, leading `/`, trailing `/`, `!`) relative to the pair root, plus the default excludes (`~$*`, `.~lock.*#`, `*.tmp`, editor swap files, the daemon's temp prefix), applied before anything else.
- A settler that keeps a path dirty until `(size, mtime)` stayed unchanged for `desktop.settle` (2 s), using timers, not polling loops.
- When the watch limit is reached (or the root is a network share), the pair runs rescan-only, logs one WARN and is listed in `DaemonStatus.watcher_degraded`.
- The `desktop.settle`, `desktop.rescan_interval` and `desktop.wake_gap` rows added to helper 01's key table.

## Scope

### Included
- Watcher, scanner, rule matcher and settler per pair, feeding settled paths to the decide step (E-24/c10a).
- Rule parsing with `RulesInvalid{line}` for the first bad line; live rule replacement (used by `update_pair`).
- The watcher-degraded state per pair and its reason (watch limit, network share, watcher error that persists).
- Config rows for the three keys (helper 01 owns the table; this task adds the rows).

### Excluded
- Deciding update, move or delete from a settled path (E-24/c10a, E-24/c10b); lock retry (E-24/c14).
- Feed rows (E-24/c12a); `DaemonStatus` assembly (E-24/c19).

## Acceptance Criteria

**AC-1: Settle before upload (AC-18)**
Given `desktop.settle` 400 ms in the config mock and hints for `a.bin` at 0, 100, 200 and 300 ms with a growing size, then silence
When the settler runs on `ManualClock`
Then exactly one settled path is released, at or after 700 ms from the first hint

**AC-2: Production settle with a slow writer**
Given the production 2 s settle and DS-27 (2 GiB written in 16 MiB steps with 1 s pauses)
When the writer runs in a pair
Then the first upload byte reaches the proxy at or after the last write plus 2.0 s, and there are 0 uploads before the writer ends (IT-14-1)

**AC-3: Gitignore grammar and defaults (AC-66)**
Given rules `*.log`, `build/`, `/root-only.txt`, `!keep.log`, `docs/**/draft?.md`
When the paths `a.log`, `keep.log`, `build/x`, `src/build/x`, `root-only.txt`, `sub/root-only.txt`, `docs/a/b/draft1.md`, `docs/draft12.md` are matched
Then `a.log`, `build/x`, `src/build/x`, `root-only.txt` and `docs/a/b/draft1.md` are excluded, `keep.log`, `sub/root-only.txt` and `docs/draft12.md` are kept, and the default excludes hold

**AC-4: Excluded paths are never touched**
Given an excluded server file and an excluded local file that is removed on the server
When the pair syncs
Then the excluded server file is never downloaded and the excluded local file is never deleted, uploaded or trashed

**AC-5: Invalid rules**
Given rule text whose line 3 is invalid
When the rules are parsed (as at `add_pair` or `update_pair`)
Then the result is `RulesInvalid{line: 3}` and the previous rules stay active

**AC-6: One rescan per trigger (AC-35)**
Given a pair on `ManualClock`
When the daemon starts, `desktop.rescan_interval` elapses, the clock jumps 30 minutes (above `desktop.wake_gap`), and the watcher reports an error
Then exactly one rescan runs per trigger, and a change made while no event was delivered is found by the rescan

**AC-7: Watch limit falls back to rescan-only (NFT-RES-40)**
Given Linux with `fs.inotify.max_user_watches` at 512 and a pair on DS-11 (100,000 files), `desktop.rescan_interval` 60 s
When the daemon starts the pair
Then within 60 s the pair is listed in `watcher_degraded` with the watch limit named, one WARN line is logged, 10 creates, 10 edits and 10 deletes are on the server within 70 s, and average idle CPU over 10 minutes is at most 1% of one core

**AC-8: Idle cost**
Given a pair with 100,000 files and no changes
When the daemon idles
Then no periodic work runs faster than the rescan timer, so idle CPU stays within AC-15 (measured in PT-14-1)

## Non-Functional Requirements

**Performance**
- A warm rescan of 100,000 files takes about 0.3 s on Linux (14 §5); the first scan finishes within 60 s (AC-15, PT-14-1).

**Reliability**
- Watcher loss of events never loses a change: every change is found by the next rescan.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Settler with 400 ms settle and four growing hints (IT-14-1 unit) | One release at >= 700 ms |
| AC-3 | Rule matrix of IT-14-4 and the default excludes | Excluded and kept sets exact |
| AC-5 | Invalid line 3 | `RulesInvalid{line: 3}`; old rules kept |
| AC-6 | `ManualClock` triggers: start, interval, 30 min jump, watcher error (IT-14-12 unit) | One rescan per trigger |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | DS-27 slow writer, production settle | IT-14-1 | First upload byte >= last write + 2.0 s | AC-18, FT-P-52 |
| AC-3, AC-4 | IT-14-4 rules and paths; excluded server and local files | IT-14-4 | 0 downloads, 0 deletes of excluded paths | AC-66, FT-P-60 |
| AC-6 | DS-23, 500 files changed locally and on device B, 30-minute guest suspend | IT-14-12, AT-14-3 step 3 | One rescan after wake; devices equal within 60 s | AC-35, FT-P-58, NFT-RES-16 |
| AC-7 | LIN-VM, `max_user_watches` 512, DS-11, rescan 60 s | NFT-RES-40 | Rescan-only reported 1 of 1; 30 of 30 changes within 70 s; idle CPU <= 1% | AC-15 |

## Runtime Completeness

- Named capability: notify 8.2.1 `RecommendedWatcher` (recursive) and the `ignore` crate's gitignore matcher (14 §5 steps 1-2).
- Production code that must exist: real OS watchers per platform and the real matcher.
- Allowed external stubs: `ManualClock` and a config mock with short durations in unit tests.
- Unacceptable substitutes: an own pattern matcher, trusting event kinds, or a debouncer used as the settle check.

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- Suspend runs on LIN-VM (`dompmsuspend`) and Windows sleep; a paused VM is weaker evidence and is labelled.
- No paths of user files above DEBUG in logs (14 §9).

## Risks & Mitigation

**Risk 1: Rescan cost on large trees (R28)**
- *Risk*: rescan-only pairs miss AC-5 and AC-35 timing.
- *Mitigation*: status shows `watcher_degraded`; the `max_user_watches` runbook (E-24 Definition of Done) tells the user how to raise the limit.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| A "shorter rescan for such pairs" (R28 mitigation) has no interval value; NFT-RES-40 sets 60 s by config | E-24 | open | Low |
| `desktop.wake_gap` is not in helper 01's table and not flat snake case (sc_config open point 1); this task adds the row | E-01 | open | Low |
| Gate S2 (E-24/c01) may change the watcher approach before this task starts | E-24 | open | Medium |
