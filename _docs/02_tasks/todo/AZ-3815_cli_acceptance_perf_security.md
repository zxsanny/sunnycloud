# Acceptance, performance and security tests of the operator CLI (AT-12-1..2, PT-12-1..2, ST-12-1..4)

**Task**: AZ-3815_cli_acceptance_perf_security
**Name**: CLI acceptance, performance and security tests
**Description**: The install check and the restore drill on the Compose test stack, command latencies at scale, scheduler overhead, and the four security tests of the CLI with real components
**Complexity**: 5 points
**Dependencies**: AZ-3813_cli_integration_suite, AZ-3654_pipeline_first_run_bootstrap, AZ-3666_pipeline_db_restore_resync, AZ-3742_test_harness_skeleton, AZ-3761_ci_skeleton
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3815
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — commands and exit codes (E-22/c02).

## Problem

The CLI's own suite uses recording ports. The operator's real promises need the real stack: a fresh install reaches `doctor` exit 0 (AC-22), the restore drill passes and skipping the bump fails (AC-41, AC-65), commands stay fast with 500 accounts and 100,000 revisions, the scheduler stays cheap, secrets never leak, destructive commands never run unconfirmed, the CLI has no listener and no SQL, and the API role cannot change the schema.

## Outcome

- AT-12-1 and AT-12-2 pass on the Compose test stack in CI.
- PT-12-1 and PT-12-2 report every metric against its target and failure threshold.
- ST-12-1..4 pass.

## Scope

### Included
- AT-12-1, AT-12-2, PT-12-1, PT-12-2, ST-12-1, ST-12-2, ST-12-3 (including the CI grep of the crate for `sqlx` and `blobs/` and the single system-scope module rule), ST-12-4.

### Excluded
- The 30-minute README install walk (FT-P-69, NFT-PERF-30: manual, E-31), the system-wide role test NFT-SEC-46 and the restore scenarios NFT-RES-07, NFT-RES-13 (E-31).

## Acceptance Criteria

**AC-1: Install check (AT-12-1, AC-22)**
Given a fresh Compose stack from the release files with one provider configured
When `doctor --pre-flight` runs before start, then the stack starts, `admin setup-token` runs and the first sign-in completes, then `doctor` runs
Then pre-flight prints the exact redirect URI of the provider and reports config, key length and volume Ok, the account is created, and `doctor` exits 0

**AC-2: Restore drill (AT-12-2, AC-41, AC-65)**
Given the DS-22 backup set restored (database, blobs, same key) on a clean stack
When `serve` starts without a bump, `doctor` runs, `admin epoch-bump --yes` runs, `scrub run --full` runs, `doctor` runs, and `serve` starts
Then the first start refuses naming `admin epoch-bump` and `doctor` fails on the marker; the bump prints the new epoch and findings; scrub exits 0; `doctor` exits 0 or 1; `serve` reaches ready and devices get `resync_required` (NFT-RES-07)

**AC-3: Command latencies at scale (PT-12-1)**
Given DS-29 (500 accounts) and the DS-11 tree (100,000 files)
When `admin user list`, `doctor`, `doctor --json`, `scrub status`, `admin media-stats` and `serve` to ready run serially
Then `admin user list` takes at most 1 s (failure above 3 s), `doctor` 60 s (180 s), `scrub status` 2 s (10 s), `serve` to `/readyz` 200 10 s (30 s), and the API container stays under 1 GiB during `doctor`

**AC-4: Scheduler overhead (PT-12-2, AC-68)**
Given 20 active accounts and production intervals for 10 minutes with no traffic
When CPU, queries and cycle times are measured
Then API container CPU is at most 2% (failure above 5%), queries per idle janitor cycle 3 or fewer (failure above 10), cycle p95 20 ms (failure above 100 ms), and the scheduler uses at most 2 database connections

**AC-5: Secrets never leave the secret channel (ST-12-1, NFT-SEC-38)**
Given known tokens and master keys used by the IT-12-2, IT-12-12 and IT-12-13 flows on the stack
When stdout of non-secret commands, all stderr, container logs (text and JSON), audit rows, `doctor --json` and host `ps` output are searched
Then each token appears exactly once (the creating command's stdout) and no key appears anywhere

**AC-6: Destructive commands need confirmation (ST-12-2)**
Given stdin from `/dev/null` and no `--yes`
When `admin user delete`, `admin rewrap`, `admin epoch-bump` and `migrate revert` run on the stack
Then each exits 3 and 0 state changes are found; with a TTY the confirmation names the target

**AC-7: No listener and no SQL of its own (ST-12-3)**
Given `doctor` and `scrub run` running, and the crate sources
When `ss -ltnp` snapshots are taken and the CI grep runs
Then no new listening socket belongs to a CLI process, the crate contains no `sqlx` use and no `blobs/` path string, and system-scope database access goes only through helper 07's single repository module

**AC-8: Database role boundaries (ST-12-4)**
Given the api container
When `DROP TABLE` is tried through its configured connection string, and its environment and mounted secrets are read
Then the statement is denied and no `db_migrator_url` is present

## Non-Functional Requirements

**Performance**
- Targets and thresholds as in AC-3 and AC-4 (PT-12-1, PT-12-2).

**Reliability**
- AT-12-2 runs in CI (epic Definition of Done).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-7 | CI grep rule on a seeded `sqlx` line | Grep fails the build |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Fresh stack, one provider | AT-12-1 | Redirect URI printed; `doctor` 0 | AC-22, FT-P-69 |
| AC-2 | DS-22 backup set | AT-12-2 | Refusal, bump, scrub 0, doctor 0 or 1, ready | AC-41, AC-65, NFT-RES-13 |
| AC-3 | DS-29, DS-11 | PT-12-1 | Within targets | NFT-PERF-30 |
| AC-4 | 20 accounts, 10 min idle | PT-12-2 | Within targets | AC-68 |
| AC-5 | Known tokens and keys | ST-12-1 | Tokens once; keys never | NFT-SEC-38 |
| AC-6 | stdin `/dev/null` | ST-12-2 | Exit 3; 0 changes | AC-27 |
| AC-7 | Running CLI processes; sources | ST-12-3 | No socket; grep clean | |
| AC-8 | api container | ST-12-4 | Denied; no migrator secret | NFT-SEC-46 |

## Constraints

- Real stack: PostgreSQL 18 with both roles, XFS blob volume, real binaries; harness superuser access only to arrange faults and assert.
- PT targets are proposed in `tests.md`; report measured values even when they pass.

## Risks & Mitigation

**Risk 1: Restore drill flakes on timing**
- *Risk*: a slow `pg_restore` pushes the drill past the CI budget.
- *Mitigation*: DS-22 is generated at a fixed size; the drill reports each step's time.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The common brief says a component task never depends on a pipeline task, but AZ-3654 (E-22/p01) and AZ-3666 (E-22/p13) name E-22/c17 as their downstream; the dependencies keep both because AT-12-1 and AT-12-2 need the proven flows | Parent (Step 4) | open | Low |
| PT-12-1 targets are proposed by `tests.md`, not by the description | E-22 owner | accepted-risk | Low |
