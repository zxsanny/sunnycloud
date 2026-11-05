# Test-only migration source spike (G-12-1)

**Task**: AZ-3787_migration_test_source_spike
**Name**: Test-only migration source spike
**Description**: Decide how the test image supplies failing, killed and irreversible migrations without changing the release binary, and record it so IT-12-11 is no longer blocked
**Complexity**: 2 points
**Dependencies**: AZ-3783_sc_db_migrator_accounts_base; notes: documents only reads the spec and `_docs/02_document/contracts/shared_db/sc_db_migrations.md`
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3787
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c01

## Problem

The release binary embeds its migrations. IT-12-11 needs a migration that fails, one that runs long enough to be killed, and a `down` that raises `irreversible: restore from backup`. No document names a way to supply them, so IT-12-11 is BLOCKED (gap G-12-1) and the rollback promise of AC-65 is tested only through real release migrations.

## Outcome

- A written decision in `components/12_operator_cli/description.md` §4 that names the mechanism for test migrations, the build or image that carries it, how a release build behaves if the option is used, and the CI check that proves the release image lacks it.
- IT-12-11 in `components/12_operator_cli/tests.md` rewritten with input data reachable through the mechanism and without the BLOCKED marker.
- The G-12-1 row of `tests/spec-gaps.md` marked resolved with the date.
- Any work the decision needs outside E-22 listed with its owning epic (E-06 for helper 07) for the parent to add; work inside E-22 noted on E-22/c07.

## Scope

### Included
- Compare the options already named in the documents: a migrations directory honoured only by the test image (IT-12-11 note), or a test build feature of the binary like the `test-clock` feature of 11 (`canonical-names.md` Runtime).
- Judge each against: the release image must not carry it (a CI check on the pushed digest, like the test-clock 404 check), applied-file immutability and checksums, and what `serve`'s `check_version` reports after test migrations ran.
- Update the three documents named in Outcome.

### Excluded
- G-12-3 (reset of the sole admin): E-15/c01 records the owner's answer in both `01_identity/description.md` and `12_operator_cli/description.md`, so one answer serves both epics; E-22/c08 consumes it.
- Building the mechanism (E-22/c07, or E-06 if it lives in helper 07).
- Migration content and the `Migrator` itself (E-06/c01b).

## Acceptance Criteria

**AC-1: Mechanism decided**
Given the two options above
When the spike ends
Then 12's description §4 names the chosen mechanism, the image or build that carries it, and why the other option was rejected

**AC-2: Release safety stated**
Given the chosen mechanism
When the decision is read
Then it states what a release build does when the option is used, and names the CI step that asserts its absence on the pushed release digest

**AC-3: IT-12-11 runnable**
Given the decision
When `tests.md` IT-12-11 is read
Then its input data (failing statement, long statement to kill, irreversible `down`) can be supplied through the mechanism, the BLOCKED marker is gone, and the G-12-1 row in `spec-gaps.md` is marked resolved with the date

**AC-4: Follow-ups placed**
Given the decision needs work in helper 07 or in 12
When the spike ends
Then each piece of work is listed with its owning epic and ref (E-06 or E-22/c07), and no work is moved across epics

## Non-Functional Requirements

**Compatibility**
- The release binary keeps embedding only the shipped migrations.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1..AC-4 | None (documents only) | n/a |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Updated 12 description §4 | Review | Mechanism, carrier, release behaviour and CI check named | |
| AC-3 | Updated `tests.md`, `spec-gaps.md` | Review of IT-12-11 and G-12-1 | Input data reachable; not BLOCKED; gap resolved with date | NFT-RES-14 |
| AC-4 | Decision text | Review | Follow-ups listed with owner epics | |

## Constraints

- Documents only; no code.
- The decision must not weaken the release path: `serve` still checks, never migrates, and `migrate` still uses only `db_migrator_url`.

## Risks & Mitigation

**Risk 1: The mechanism lives in helper 07**
- *Risk*: the cleanest place is `sc_db`'s migrator, which is E-06's.
- *Mitigation*: AC-4 hands the work to E-06 by ref instead of doing it in E-22.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-12-1: no test-only migration source; IT-12-11 BLOCKED until this spike ends | This task | open | Medium |
| G-12-3 is listed in this row's title and in E-15/c01; E-15/c01 records the single answer in both descriptions, this task does not ask it again (closes the duplicate concern on AZ-3681) | E-15/c01 / product owner | open | Low |
