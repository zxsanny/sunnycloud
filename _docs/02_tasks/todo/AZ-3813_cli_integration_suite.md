# CLI integration suite IT-12-1..19 with recording owner ports

**Task**: AZ-3813_cli_integration_suite
**Name**: CLI integration suite
**Description**: Run the whole IT-12 suite in CI at the CLI boundary, own the cross-command cases, assert one business call per command for every command, enforce the 90% coverage floor, and map every owner-effect expectation to the task that proves it
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3790_per_command_config, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3793_migrate_commands, AZ-3794_identity_operator_commands, AZ-3795_user_seed_audit_commands, AZ-3796_epoch_restore_gc_commands, AZ-3797_key_commands, AZ-3798_scrub_commands, AZ-3805_media_commands, AZ-3807_doctor_aggregation, AZ-3809_doctor_root_checks, AZ-3811_healthcheck_openapi_version, AZ-3742_test_harness_skeleton, AZ-3761_ci_skeleton
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3813
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — the command table the single-call check walks (E-22/c02).

## Problem

Each command task tests its own IT-12 case with the harness of E-22/c02. What no single task proves: that every command class follows the same exit and JSON rules, that no command makes zero or two business calls, that a secret never leaks across commands, and that the suite runs as one CI gate. The IT-12 cases also contain expectations that only real owners can meet (rows, files, feed changes); each must be proven somewhere, not dropped.

## Outcome

- One CI job runs IT-12-1..19 (the per-command cases of E-22/c05..c15 and the cases below).
- IT-12-1 as a matrix over every command class: usage error, missing confirmation, owner refusal, doctor levels, `--json`.
- A table-driven check over the contract's command table: each command has a test that asserts exactly one business operation (a loop repeats the same operation); a command added to the table without a row fails the check.
- IT-12-18 (lazy construction) and IT-12-19 (secret channel and cancellation) across commands.
- Coverage of the exit-code mapping, the confirmation guard, the secret writer and the start-up order at least 90%.
- A coverage map: every expected result of IT-12-1..19 is assigned to this suite, a command task, a pipeline, an owner task or E-31.

Narrowed against the plan (5 points): the per-command IT-12 cases are written by their command tasks with the E-22/c02 harness, so this task keeps the cross-command cases, the gate and the map (3 points).

## Scope

### Included
- The CI job, the cross-command cases, the single-call check, the coverage gate and the coverage map (kept beside the suite).

### Excluded
- Per-command cases (E-22/c05..c15), real-stack acceptance, performance and security tests (E-22/c17), end-to-end flows (E-22/p01..p14).

## Acceptance Criteria

**AC-1: Exit and JSON rules for every command class (IT-12-1)**
Given recording owner ports
When an unknown command, a missing argument, `admin user delete <id>` without `--yes` on a non-TTY and with it, `admin rewrap` and `migrate revert` without `--yes`, `doctor` healthy, with one warning and with one failure, and `--json` on each run
Then usage and missing confirmation exit 3 with no state change, doctor exits 0, 1 and 2, owner refusals exit 2, and each `--json` run prints one object `{result, code, data}` with the owner's code

**AC-2: One business call per command**
Given the command table of the contract
When the single-call check runs
Then every command has a test that sees exactly one business operation on its port, and removing one command's test, or adding a command without one, makes the check fail

**AC-3: Secret channel across commands (IT-12-19, ST-12-1)**
Given `admin create`, `admin setup-token`, `admin reset` and `admin invite create` with stdout and stderr captured apart and `log = json`
When each runs
Then each token is on stdout exactly once, and 0 times in stderr, log lines and the output of the other commands

**AC-4: Cancellation (IT-12-19)**
Given `scrub run --full` and `admin rewrap --yes` with blocking recording ports
When SIGINT is sent at 30% progress
Then each stops within 5 s, the scrub run is closed with `finish_run`, the rewrap starts no new batch, and the exit code is recorded in the test report (G-12-7)

**AC-5: Lazy construction (IT-12-18)**
Given no provider and no worker token configured
When `migrate`, `admin setup-token` and `key-check` run, and `openapi` runs with no database and no key
Then all succeed

**AC-6: Coverage gate**
Given the suite and the unit tests
When CI measures coverage
Then the exit-code mapping, the confirmation guard, the secret writer and the start-up order each reach at least 90%, otherwise the job fails

**AC-7: Coverage map complete**
Given IT-12-1..19 in `components/12_operator_cli/tests.md`
When the coverage map is reviewed
Then every expected result is assigned to exactly one place (this suite, E-22/c05..c15, E-22/p01..p14, an owner task or E-31), and none is unassigned

## Non-Functional Requirements

**Performance**
- The suite finishes within the sum of the IT-12 maximum execution times (1,580 s).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Single-call check against a table with a missing row | Check fails |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Harness, recording ports, stdin `/dev/null` | IT-12-1 matrix | 3 / 0 / 1 / 2 as stated; one JSON object | |
| AC-2 | Contract command table | Single-call check | One operation per command | |
| AC-3 | Harness, `log = json` | IT-12-19 secret part | Token once on stdout only | NFT-SEC-38 |
| AC-4 | Harness, blocking ports | IT-12-19 SIGINT part | Stops within 5 s; scrub closed | |
| AC-5 | Missing unrelated config | IT-12-18 lazy part | All succeed | |
| AC-6 | CI coverage | Gate | At least 90% on the four areas | |

Owner-effect parts and where they are proven (the map of AC-7 starts from this table):

| IT-12 case | Owner-effect expectation | Proven by |
|------------|--------------------------|-----------|
| IT-12-2 | Old setup token refused; only SHA-256 stored; sessions revoked after reset | AZ-3654 (E-22/p01) AC-2, AC-4; E-15/c08 |
| IT-12-3 | Concurrent demotes give {0, 2}; quota visible to the user; U purged to 0 rows | E-17/c04; FT-N-46; AZ-3665 (E-22/p12) AC-2 |
| IT-12-4 | Content hash back, feed change, `revisions` count unchanged | AZ-3663 (E-22/p11) AC-3; E-13/c11 |
| IT-12-5 | Marker equals the epoch; devices get `resync_required` | AZ-3666 (E-22/p13) AC-2; NFT-RES-07 |
| IT-12-6 a, c, d, i | Pending migration, epoch below marker, wrong key, no provider | AZ-3654 (E-22/p01) AC-3; AZ-3666 (E-22/p13) AC-1, AC-72 |
| IT-12-7 | Every account table empty after deletion; derived rows gone after GC | AZ-3665 (E-22/p12) AC-2; AZ-3663 (E-22/p11) AC-2 |
| IT-12-9 | `gc-run` beside the scheduler and scrub: no double delete, no false finding | AZ-3663 (E-22/p11); AZ-3667 (E-22/p14) AC-80 |
| IT-12-12, IT-12-13 | `.keycheck` not created on a wrong key; blob files unchanged by rewrap | E-10/c05, E-10/c13; FT-P-41 |
| IT-12-14 | Flipped bit found; `--rate` timing; safe beside uploads and GC | E-10/c12; AZ-3667 (E-22/p14) |
| IT-12-15 | Listings 200 and 404 `content_unavailable` for missing blobs | AZ-3666 (E-22/p13) AC-73 |
| IT-12-16 | Requeued rows `queued`; blob removed after retention | E-18/c05; AZ-3663 (E-22/p11) AC-1 |
| IT-12-17 | One `admin_action` row per mutation; refused seed writes nothing | E-17/c03, E-17/c08; E-31 |

## Constraints

- Recording owner ports only replace business components; `migrate` cases use real PostgreSQL 18 (helper, not a port).

## Risks & Mitigation

**Risk 1: An owner-effect expectation is lost between tasks**
- *Risk*: a case assumes a pipeline covers it, and the pipeline does not.
- *Mitigation*: AC-7 requires a named owner for every expectation.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Exit code of a long command cancelled by SIGINT is not defined (G-12-7 covers `serve`); AC-4 records it | E-22 owner | open | Low |
| IT-12-11 stays BLOCKED in this suite until E-22/c01 and E-22/c07 land the test migration source (G-12-1) | E-22/c01 | open | Medium |
