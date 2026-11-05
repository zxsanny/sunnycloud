# Database restore, epoch bump and resync pipeline (F14)

**Task**: AZ-3666_pipeline_db_restore_resync
**Name**: Restore, epoch and resync pipeline
**Description**: After a database restore, `serve` refuses until `admin epoch-bump`, `doctor` reports missing blobs, and devices get `resync_required`, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3791_serve_start_stop, AZ-3796_epoch_restore_gc_commands, AZ-3797_key_commands, AZ-3807_doctor_aggregation, AZ-3809_doctor_root_checks, AZ-3776_change_feed_read, AZ-3779_epoch_admin, AZ-3725_master_keyring, AZ-3746_blob_maintenance_ports, AZ-3707_tree_reader, AZ-3716_tree_revision_lookups, AZ-3755_transfer_download, AZ-3829_tree_transfer_feed_handlers, AZ-3658_pipeline_change_feed, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3666
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p13

## Problem

Restore safety depends on start-up order in `serve` (schema check, master-key check, epoch-marker check before any listener), on `admin epoch-bump` calling 05 which writes 04's `.epoch` marker, on the restore-tagged `DoctorCheck`s registered by 12 (04 inventory mapped to paths by 02), and on 05's cursor rules reaching devices through 11. E-22's own integration suite uses fake owner ports, and the restore drill (AT-12-2) is a test task that owns no wiring. No planned task proves the sequence on the real binary.

## Outcome

- A restored database cannot be served until the epoch is bumped; after the bump devices get `resync_required` and the listing works.
- `doctor` lists every revision whose blob is missing while users see those files as unavailable, never as server errors.
- A wrong master key stops `serve` before any listener.
- Every component operation of flow F14 has a production caller; at least one integration test runs F14 through the real binaries.

## Scope

### Included
- Flow order and seams (`system-flows.md` F14 steps 1-7, server side):
  1. Operator restores a dump (test: `pg_restore` of an earlier dump).
  2. `serve` → `Migrator::check_version` → `MasterKeyring::load_and_verify` → `EpochAdmin::marker_status` → refuse when the database epoch is below the marker, naming `admin epoch-bump`.
  3. `admin epoch-bump` → `EpochAdmin::bump_epoch(now)` → `EpochMarker::write` after commit → restore-tagged doctor checks: `BlobInventory::find_missing` mapped through `TreeReader::describe_blobs`, epoch check.
  4. `serve` starts → device poll with the old cursor → `ChangeFeed::read` → 410 `resync_required` (`epoch_changed`, or `cursor_ahead`).
  5. Device reads `ChangeFeed::head` and lists the tree in pages; missing-blob files list as unavailable and download as content-unavailable 404.
  6. `admin keycheck-init` for a restored volume with a confirmed key; `key-check`.
- The end-to-end integration test of the steps above. The device-side merge is E-24/p03.

### Excluded
- Backup tooling (operator), the full CI restore drill with scrub (E-22/c17 AT-12-2, which should depend on this task), client merge (E-24/p03).

## Acceptance Criteria

AC-1..AC-4 are local to this task. AC-73 and AC-72 (key-check part) keep their numbers from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Refusal until the bump**
Given a database restored from an earlier dump onto a blob volume whose `.epoch` marker is newer
When `sunnycloud serve` starts, and `doctor` runs
Then `serve` exits non-zero before any listener binds and prints `admin epoch-bump`, and `doctor` reports Fail for the epoch check

**AC-2: Bump then resync**
Given the restored install
When `admin epoch-bump` runs, `serve` starts, and a device polls with its pre-restore cursor
Then the marker holds the new epoch, `serve` reaches ready, and the poll gets 410 `resync_required` with `epoch_changed`; a cursor above head on the same epoch gets `cursor_ahead`

**AC-3: Paged listing after resync**
Given the bumped install
When the device reads the head cursor and lists the tree in pages
Then every live node appears once and the device can resume polling from the head

**AC-4: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `EpochAdmin::{bump_epoch, marker_status}` (command and `serve`), `EpochMarker::write` (from 05), `MasterKeyring::{load_and_verify, init_key_check}`, `BlobInventory::find_missing` and `TreeReader::describe_blobs` (restore-tagged doctor checks registered by 12), and the start-up order schema check, key check, marker check, listeners

### Interaction criteria (Step 1.8)

**AC-73: Missing blobs are reported, never served as errors** (R6)
Given a restored database whose rows reference three blob files that are absent from the volume
When `doctor` runs, and a user lists the folders and opens those files
Then `doctor` lists each revision with path, account and revision id and exits 2 (or 1 by policy), the listing returns 200 with those files marked unavailable, and opening one returns 404 content-unavailable, never a 5xx

**AC-72 (key-check part): A wrong master key fails closed** (R5)
Given a blob volume with a stored key check
When `serve` starts with a different master key, and `key-check` runs with it
Then `serve` exits non-zero before any listener binds, `key-check` exits 2 with `KeyCheckMismatch`, and no content byte is served
(The worker-fetch part of AC-72 is owned by E-22/p08.)

## Non-Functional Requirements

**Performance**
- `doctor` full run within 60 s on the test data (PT-12-1).

**Reliability**
- The marker only ever rises; a failed marker write makes `epoch-bump` return the error so the operator reruns it.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Static caller check over non-test sources | Each named call has a production caller; start-up order as specified |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Restored DB, newer marker | `serve`, `doctor` | Refusal naming `admin epoch-bump`; doctor Fail | AC-65 |
| AC-2 | Restored install | `epoch-bump`, `serve`, old cursor poll | Marker updated, ready, 410 `epoch_changed`; `cursor_ahead` | AC-41 |
| AC-3 | Bumped install | Head, paged listing | Each live node once | AC-6 |
| AC-73 | Three referenced blob files absent | `doctor`, list, open | Listed with ids; listing 200; 404 content-unavailable | NFT-RES-37 |
| AC-72 | Stored key check, other key | `serve`, `key-check` | Exit before listeners; exit 2 `KeyCheckMismatch` | FT-N-43 |

## Constraints

- Owner: component 12 (`src/server/`): start-up order, command and doctor-check wiring for this flow, and its end-to-end tests among 12's integration tests. No SQL or blob I/O in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 (real dump and restore) and a real blob volume; no sunnycloud component is faked.
- Downstream: E-22/c17 (AT-12-2), E-31/c24 (FT-P-13, FT-P-14), E-31/c30 (FT-P-71), E-31/c32 (FT-N-33), E-31/c33 (FT-N-43), E-31/c39 and E-31/c41 (NFT-RES-37) should depend on this task.

## Risks & Mitigation

**Risk 1: Listener binds before the checks**
- *Risk*: a refactor of `serve` starts the HTTP server before the key or epoch check.
- *Mitigation*: AC-1 and AC-72 assert no port is open when `serve` exits.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-72 is split: fetch part in E-22/p08, key-check part here | Step 1.8 mapping | accepted-risk | Low |
