# `doctor` aggregation: registry, levels, `--pre-flight`, `--json`, exit codes

**Task**: AZ-3807_doctor_aggregation
**Name**: Doctor aggregation
**Description**: Run every registered `DoctorCheck` (or a tagged subset), print each finding with its level, message and remedy in text or JSON, and exit 0 ok, 1 warnings only, 2 any failure
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3768_contracts_part1
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3807
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c14a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DoctorCheck`, `DoctorTag`, `DoctorContext`, `Finding`.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — `doctor` row and exit codes (E-22/c02).

## Problem

`doctor` is the install check (AC-22) and the restore-drill verdict (AC-65): scripts read its exit code, and the operator reads its remedies. The checks are implemented by their owners behind the `DoctorCheck` port; 12 only runs them and reports. `--pre-flight` runs before the stack is up, so it must not need the database.

## Outcome

- `doctor` runs every registered check with a `DoctorContext` built from `--pre-flight`, `--backup-interval <duration>` (for example `24h`) and `--attest-backup`.
- Each finding prints with its check name, level (Ok, Warn, Fail), message and remedy; `--json` prints one object with identical content.
- Exit 0 when every finding is Ok, 1 when there are warnings and no failure, 2 on any Fail (gap-pass default G-12-6).
- `--pre-flight` runs only the checks tagged `pre-flight` and opens no database connection.
- Other commands can run a tagged subset through the same aggregation (`admin epoch-bump` runs the `restore`-tagged checks, E-22/c10).
- `doctor` is read-only and prints no secret.

Split from the planned row "doctor aggregation: registry, levels, `--pre-flight`, `--json`, exit codes" (5 points): the aggregation is here (3 points); the checks that only 12 can provide are E-22/c14b (3 points). Together they are above 5 points, so the row is split.

## Scope

### Included
- Running the registered checks or a tagged subset, building the context, printing, the JSON form and the exit mapping.

### Excluded
- Every check's logic (owners: 04, 05, 07a, 08 and helpers through E-22/c14b), registration of the checks (E-22/c03), the 12-owned checks (E-22/c14b).

## Acceptance Criteria

**AC-1: Levels to exit codes (IT-12-1, IT-12-15)**
Given recording checks that return only Ok, then one Warn, then one Warn and one Fail
When `doctor` runs for each
Then the exit codes are 0, 1 and 2, and each finding prints with check name, level, message and remedy

**AC-2: Same content in JSON (IT-12-15)**
Given the checks of AC-1
When `doctor --json` runs
Then stdout is one JSON object whose findings, levels and remedies equal the text output, and the exit code is the same

**AC-3: Pre-flight without a database (AC-22, IT-12-15)**
Given checks tagged `pre-flight` and others that are not, and no database running
When `doctor --pre-flight` runs
Then only the `pre-flight` checks run, no database connection is attempted, and their findings (including the providers' redirect URIs) are printed

**AC-4: Tagged subset for other commands**
Given checks tagged `restore` and others
When the restore subset is requested by `admin epoch-bump`
Then only the `restore` checks run and their findings return to the caller

**AC-5: Options reach the checks**
Given `--backup-interval 7d --attest-backup`, and separately `--backup-interval soon`
When `doctor` runs
Then every check receives a context with a 7-day backup interval and the attest flag set, and the malformed value exits 3 with no check run

**AC-6: Read-only, no secrets (IT-12-15, NFT-SEC-38)**
Given the full stack with known secrets
When `doctor` and `doctor --json` run
Then table insert, update and delete counters do not change (apart from an audit row if the run is audited), and no secret, token, key byte or database password appears in the output

## Non-Functional Requirements

**Performance**
- `doctor` full: target 60 s, failure above 180 s (PT-12-1, AC-22), measured in E-22/c17.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Level aggregation | 0 / 1 / 2 |
| AC-2 | JSON against text | Same findings |
| AC-3, AC-4 | Tag selection | Only tagged checks run |
| AC-5 | Context building and duration parsing | Options passed; bad duration exit 3 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, recording checks | `doctor`, `doctor --json` (IT-12-1, IT-12-15) | 0 / 1 / 2; same content | |
| AC-3 | Harness, database stopped | `doctor --pre-flight` (IT-12-15) | Pre-flight checks only; no connection | AC-22 |
| AC-6 | Full stack, known secrets | `doctor` read-only and secret search (IT-12-15) | Counters unchanged; 0 secrets | NFT-SEC-38 |

The levels of each fault in IT-12-15 (missing blobs, drift, marker, worker, image age, wrong key, role attributes, retention) are produced by the checks of E-22/c14b and the owners; the restore case end to end is AZ-3666 (E-22/p13) AC-73.

## Constraints

- Aggregation only; no check logic in this task.

## Risks & Mitigation

**Risk 1: One slow check stalls `doctor`**
- *Risk*: a provider probe hangs and `doctor` exceeds 180 s.
- *Mitigation*: owners bound their own probes (01 providers within 10 s per `observability.md`); see the flagged concern on a per-check timeout.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether `doctor` applies a per-check timeout or isolates a check that panics is not stated (07's health aggregation has a per-check timeout) | E-22 owner | open | Low |
| Whether a `doctor` run itself writes an audit row is not stated (IT-12-15 allows it) | E-22 owner | open | Low |
