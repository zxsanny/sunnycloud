# EpochAdmin: epoch bump, marker status, head report and the epoch doctor check

**Task**: AZ-3779_epoch_admin
**Name**: Epoch admin
**Description**: Bump the epoch of every account after a restore so all devices resync, report the database epoch against the `.epoch` marker for `serve` and `doctor`, and fail the doctor check when a bump is missing
**Complexity**: 3 points
**Dependencies**: AZ-3776_change_feed_read, AZ-3778_feed_horizon_lifecycle, AZ-3746_blob_maintenance_ports, AZ-3768_contracts_part1
**Component**: 05 sync_feed
**Tracker**: AZ-3779
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c07
**Document dependencies**: `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` (`EpochMarker`, produced by E-10/c13)

## Problem

After a database restore, devices that were ahead of the restored state would silently miss or undo changes unless every cursor becomes invalid (AC-41). The restore runbook therefore bumps the epoch (`admin epoch-bump`), and a forgotten bump must be detectable: `serve` refuses to start when the database epoch is below the marker and `doctor` reports Fail (R02, AC-65).

## Outcome

- `EpochAdmin::bump_epoch(now)` sets `epoch = greatest(epoch + 1, unix_seconds(now))` for every feed row in one transaction, then writes the new value with `EpochMarker::write`; a marker write failure is returned after the database commit, and a rerun raises the epoch again (never lower).
- `marker_status()` returns `MarkerStatus{db_epoch, marker: Absent | Present(epoch)}` for `serve` and `doctor`; `head_report()` returns `max(epoch)` and per-account heads.
- The epoch `DoctorCheck` fails, with the remedy `admin epoch-bump`, when the marker is absent on a volume that holds blobs or differs from the database `max(epoch)`.
- INFO log for each bump with the old and new epoch.

## Scope

### Included
- `bump_epoch`, `marker_status`, `head_report`, the doctor check, the INFO log.

### Excluded
- The `admin epoch-bump` command and exit codes (E-22/c10), `serve`'s refusal (E-22/c05), doctor aggregation (E-22/c14), the restore procedure itself (E-22).

## Acceptance Criteria

**AC-1: Bump raises every row together**
Given 3 accounts with epoch E and the clock at T
When `bump_epoch(now)` runs three times, and once a run is aborted midway
Then each successful bump sets the same new epoch `greatest(E + 1, unix_seconds(T))` (then +1 each time) on all rows, the aborted run changes none, and after each success `EpochMarker::read` returns the new epoch

**AC-2: Marker failure is reported after commit and rerun is safe**
Given a read-only blob root
When `bump_epoch` runs, the root is made writable and it runs again
Then the first returns the marker error after the database commit, and the rerun succeeds, raises the epoch again and writes the marker; the epoch never decreases

**AC-3: Clock behind the epoch**
Given epoch E and a clock earlier than E seconds
When `bump_epoch` runs
Then the new epoch is E + 1

**AC-4: Old cursors resync, new cursors work**
Given cursors taken before a bump
When they are read after it
Then each returns `ResyncRequired{epoch_changed}`, a cursor `E'.0` reads normally, and `seq` values are unchanged

**AC-5: Restore without a bump is visible**
Given a database dumped at seq 10, activity up to seq 40 with the marker written, and the dump restored into a fresh database
When a device reads with `E.40` and `marker_status` is called, and then the operator bumps
Then the read returns `ResyncRequired{cursor_ahead}`, `marker_status` returns the database epoch and `Present(epoch)`, and after the bump the database epoch is above the old marker value and the marker shows the new epoch

**AC-6: Doctor check levels**
Given in turn a marker absent with blobs present, a marker of 5 with database `max(epoch)` 9, equal values, and a marker absent on an empty install
When the epoch doctor check runs
Then the findings are Fail (remedy `admin epoch-bump`), Fail, Ok and Ok

**AC-7: Restore then resync (AT-05-2)**
Given devices D1 and D2 with cursors at seq 40 and the database restored to seq 10
When D1 reads `E.40`, the operator bumps, and D1 and D2 read their old cursors and then take a new head
Then D1 first gets `cursor_ahead`, the marker is updated, both then get `epoch_changed`, and the new head cursor reads normally

**AC-8: Head report**
Given 3 accounts with different heads
When `head_report` runs
Then it returns `max(epoch)` and each account's `epoch.seq`

## Non-Functional Requirements

**Reliability**
- `bump_epoch` is idempotent in effect (a second run only raises the epoch again).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | New-epoch formula with the manual clock | `greatest(E + 1, unix_seconds(now))` |
| AC-6 | Doctor level table | Fail, Fail, Ok, Ok |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 3 accounts, clock T | IT-05-10 bump cases | Same epoch on all rows; marker follows | AC-65 |
| AC-2 | Read-only root | IT-05-10 marker failure | Error after commit; rerun raises | |
| AC-3 | Clock < E | IT-05-10 clock case | E + 1 | |
| AC-4 | Pre-bump cursors | IT-05-10 cursor case | `epoch_changed`; new cursor works | AC-41 |
| AC-5 | Dump at 10, restore after 40 | IT-05-11 | `cursor_ahead`; marker status | NFT-RES-07, FT-P-13 |
| AC-6 | Four marker states | IT-05-14 | Fail, Fail, Ok, Ok | NFT-RES-13 |
| AC-7 | D1, D2 at 40, restore to 10 | AT-05-2 | Resync then normal reads | FT-P-14 |
| AC-8 | 3 accounts | `head_report` | Max epoch and heads | |

## Constraints

- `bump_epoch`, `marker_status` and `head_report` run in system scope only (operator path, ST-05-2).
- The marker is reached only through `EpochMarker`; the bump commits before the marker write.

## Risks & Mitigation

**Risk 1: Volume and database restored to the same point (R02)**
- *Risk*: the marker matches the restored database and shows nothing.
- *Mitigation*: the `S > seq` rule catches devices that were ahead (AC-5); documented in the restore runbook.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The doctor check must know whether "the volume holds blobs", but 05 reaches 04 only through `EpochMarker` (`write`, `read`) and must not read 04's tables; the source of that fact is not named | Epic owner / E-10 | open | Medium |
| The tests of this task use the real 04 `EpochMarker`; whether the E-00 crate-graph test allows that as a dev-dependency is not stated | E-00 | open | Medium |
