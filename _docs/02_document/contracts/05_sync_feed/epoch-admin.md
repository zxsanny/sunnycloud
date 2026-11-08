# Contract: epoch-admin

**Component**: 05_sync_feed
**Producer task**: AZ-3779 (E-12/c07)
**Consumer tasks**: E-22 operator CLI (`admin epoch-bump` c10, `serve` marker check c05, doctor aggregation c14)
**Version**: 1.0.0
**Status**: draft (frozen when AZ-3779 is accepted)
**Last Updated**: 2026-11-08

## Purpose

How the epoch of the change feed is raised after a database restore and compared with the `.epoch` marker. Trait signatures (`EpochAdmin`, `MarkerStatus`, `HeadReport`, `EpochAdminError`) are declared in `contracts` part 2 (`_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`); the marker port is `EpochMarker` (`../04_blobstore/epoch-marker-and-health.md`). Source: `components/05_sync_feed/description.md` § EpochAdmin.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `PgEpochAdmin::new` | `Db, Arc<dyn EpochMarker>` -> admin | none | sync |
| `EpochAdmin::bump_epoch` | `now: Timestamp` -> `new_epoch: i64` | `Unexpected` (database failure, no feed row, marker write failure after the commit) | async, system scope |
| `EpochAdmin::marker_status` | `()` -> `MarkerStatus{db_epoch, marker: Absent \| Present(epoch)}` | `Unexpected` (database failure, marker read failure) | async, system scope |
| `EpochAdmin::head_report` | `()` -> `HeadReport{max_epoch, heads: Vec<(account, Cursor{epoch, seq})>}` ordered by account id | `Unexpected` | async, system scope |
| `epoch_findings` | `MarkerStatus, holds_blobs: bool` -> `Vec<Finding>` (one finding) | none | sync, pure |

## Invariants

- `bump_epoch` runs one statement in one system-scope transaction: every `account_feeds` row gets `greatest(max(epoch) + 1, unix_seconds(now))`, so all rows share one epoch and it never decreases. `seq` and `purge_horizon` are untouched. A failed or aborted run changes no row.
- The marker is written after the commit, through `EpochMarker::write(new_epoch)` only. A marker failure is returned as `Unexpected` with the epoch already committed; the rerun raises the epoch again and writes the marker.
- A bump on an installation with no feed row is `Unexpected(NothingToBump)`: no epoch is chosen and no marker is written.
- `marker_status.db_epoch` is `max(epoch)`, 0 when there is no feed row. The marker is read after the transaction closed.
- `epoch_findings`: marker absent and `holds_blobs` -> `Fail`; marker absent and no blobs -> `Ok`; marker equal to `db_epoch` -> `Ok`; any other marker -> `Fail`. Every `Fail` has the remedy ``run `sunnycloud admin epoch-bump` ``; messages hold epoch numbers only.
- A bump logs INFO `epoch bumped` once, without the old and new epoch: the log schema (`sc_observability` fields) has no epoch field.
- Cursor effect: a cursor of the old epoch reads as `ResyncRequired{epoch_changed}`; a cursor `E'.S` of the new epoch reads normally.

## Non-Goals

- Not covered: the `admin epoch-bump` command and its exit codes, `serve`'s refusal, doctor aggregation and the restore procedure (E-22).
- Not covered: how the doctor learns that the volume holds blobs (`holds_blobs`); no source is documented, so the `DoctorCheck` type that supplies it is not part of this version.

## Versioning Rules

- **Breaking changes** (the bump formula, the `NothingToBump` outcome, a finding level or remedy) require a new major version and notice to E-22.
- **Non-breaking additions** (the `DoctorCheck` type, log fields) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| bump-three | 3 rows at 7, clock T, three bumps | `T`, `T+1`, `T+2` on all rows; marker follows | IT-05-10 |
| bump-aborted | trigger refuses one row update | no row changed, marker untouched | IT-05-10 |
| marker-failure | marker write fails, then works | error after commit; rerun `T+1` | IT-05-10 (double; not a read-only root) |
| clock-behind | rows at `T+1000`, clock `T` | `T+1001` | IT-05-10 |
| restore-no-bump | feed row at seq 10, cursor `E.40` | `cursor_ahead`; status `Present(E)`; after bump marker is new | IT-05-11 |
| levels | absent+blobs, 5 vs 9, 9 vs 9, absent+empty | Fail, Fail, Ok, Ok | IT-05-14 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-08 | Initial contract (AZ-3779) | implement |
