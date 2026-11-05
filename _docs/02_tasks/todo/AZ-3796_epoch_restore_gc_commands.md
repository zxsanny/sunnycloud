# `admin epoch-bump`, `admin restore`, `admin gc-run`

**Task**: AZ-3796_epoch_restore_gc_commands
**Name**: Epoch-bump, restore and gc-run commands
**Description**: The restore-procedure commands: bump the epoch after a database restore and show the reconcile findings, restore one file by path and time, and run one GC cycle on demand
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3807_doctor_aggregation, AZ-3779_epoch_admin, AZ-3715_tree_restore, AZ-3730_gc_sweeper, AZ-3765_sc_clock
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3796
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `EpochAdmin`, `TreeRestore` (`account, NodeOrPath, at` -> `RestoreOutcome`; `BeyondRetention`, `FileMissing`, `NotFound`).
- `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` — `EpochMarker` semantics.
- `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md` — `GcSweeper`.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

After a database restore the epoch must rise before `serve` may start (AC-41, R02), and the operator needs to see which rows have no blob and which blobs have no row. A single deleted file must be restorable by path and time without a full restore (AC-30), and GC must be runnable on demand in the restore drill (NFT-RES-37). The commands must be safe to script.

## Outcome

- `admin epoch-bump [--yes]`: without `--yes` on a non-TTY exit 3 with no call; with it one `EpochAdmin::bump_epoch(now)` call, then the `restore`-tagged doctor checks through the doctor aggregation (E-22/c14a); prints the new epoch and the findings; exit 0 on success, 1 when the bump returns an error, including a marker write failure after commit (gap-pass default G-12-5).
- `admin restore --account --path --at <RFC 3339 UTC> [--dry-run]`: one `TreeRestore::restore_revision` call; prints `Restored{revision, placement}` with placement `Original` or `Restored`; `BeyondRetention` and `NotFound` exit 2; an `--at` that is not an RFC 3339 UTC timestamp exits 3 with no call.
- `admin gc-run`: one `GcSweeper::run_cycle(now)` call (the owner drains until nothing is due or the cycle budget ends); prints the counts; exit 0.

Narrowed against the plan: retention, restore and GC end to end are AZ-3663 (E-22/p11); the restore, epoch and resync flow is AZ-3666 (E-22/p13). This task owns the command side.

## Scope

### Included
- The three commands: parsing, the confirmation guard, the business call, the follow-up doctor run of `epoch-bump`, output and exit mapping.

### Excluded
- Epoch rules and the marker (05, 04), restore placement and retention rules (02), the GC cycle (04), the doctor checks themselves (owners, E-22/c14b).

## Acceptance Criteria

**AC-1: Epoch bump needs confirmation (IT-12-5, ST-12-2)**
Given stdin that is not a TTY
When `admin epoch-bump` runs without `--yes`
Then it exits 3 and no `bump_epoch` call is made

**AC-2: Epoch bump prints epoch and findings (IT-12-5, AT-12-2 step 2)**
Given a recording `EpochAdmin` returning a new epoch and `restore`-tagged checks reporting two rows without blobs and one orphan
When `admin epoch-bump --yes` runs
Then one `bump_epoch` call is made with the clock's current time, the restore-tagged checks run after it, the new epoch and the three findings are printed, and the exit code is 0

**AC-3: Epoch bump error exits 1 (G-12-5)**
Given a `bump_epoch` that returns an error after the database commit (marker write failed)
When `admin epoch-bump --yes` runs
Then the error is printed with the advice to run the command again, and the exit code is 1

**AC-4: Restore outcomes (IT-12-4, FT-P-18)**
Given recording `TreeRestore` answers `Restored{revision, Original}`, `Restored{revision, Restored}`, `BeyondRetention` and `NotFound`
When `admin restore --account A --path /x --at 2026-11-01T10:00:00Z` runs for each
Then the first two print the revision and placement and exit 0, and the last two exit 2 with their codes

**AC-5: Restore argument checks (IT-12-4)**
Given `--at 2026-11-01 10:00` (not RFC 3339 UTC), and a missing `--path`
When `admin restore` runs
Then each exits 3 with no business call

**AC-6: Dry run (IT-12-4)**
Given `--dry-run`
When `admin restore` runs
Then the call is made in dry-run form and the printed result says nothing was changed

**AC-7: GC on demand (IT-12-16, NFT-RES-37)**
Given a recording `GcSweeper` returning counts
When `admin gc-run` runs
Then exactly one `run_cycle` call is made, the counts are printed, and the exit code is 0 whether the counts are zero or not

## Non-Functional Requirements

**Reliability**
- A rerun of `admin epoch-bump` after an error is always safe (05 raises the epoch again, never lower).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Guard | No call on non-TTY |
| AC-2, AC-3 | Bump then restore-tagged checks; error mapping | Order bump then checks; exit 0 / 1 |
| AC-4, AC-5 | Restore result and argument mapping | Exit 0 / 2 / 3 as stated |
| AC-7 | GC count printing | One call; exit 0 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | Harness, recording `EpochAdmin` and restore-tagged checks | `epoch-bump` (IT-12-5) | Exit 3 no call; epoch and findings, exit 0; error exit 1 | AC-41 |
| AC-4..AC-6 | Harness, recording `TreeRestore` | `restore` outcomes and bad arguments (IT-12-4) | Placement printed; exit 2; exit 3 no call | AC-30, FT-P-18 |
| AC-7 | Harness, recording `GcSweeper` | `gc-run` (IT-12-16) | One call, counts, exit 0 | NFT-RES-37 |

The owner-effect parts (marker equals the new epoch, a second bump is strictly higher, restored file back with its hash and a feed change, `revisions` count unchanged, blob removed after day 7) are proven by AZ-3666 (E-22/p13) AC-2, AZ-3663 (E-22/p11) AC-1 and AC-3, and NFT-RES-07 in E-31.

## Constraints

- One business call per command; `epoch-bump`'s doctor run uses the doctor aggregation of E-22/c14a, not a second set of checks.

## Risks & Mitigation

**Risk 1: Operator skips the bump**
- *Risk*: devices that are ahead keep a stale cursor after a restore.
- *Mitigation*: `serve` refuses (E-22/c05, E-22/p13) and `doctor` fails; this command prints what to fix next.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `TreeRestore::restore_revision(account, NodeOrPath, at)` has no dry-run input (contracts part 2, E-13/c11); AC-6 needs one | E-13 owner | open | Medium |
| G-12-5 default makes `epoch-bump` exit 1 on error, while the general table uses 2 for failures; reconcile findings do not change the exit code | E-22 owner | accepted-risk | Low |
| E-13/c11 also returns `FileMissing`; this task maps it to exit 2 like the other owner refusals | E-22 owner | open | Low |
