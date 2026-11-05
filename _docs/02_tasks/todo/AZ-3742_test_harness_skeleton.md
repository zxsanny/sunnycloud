# Shared black-box test-harness skeleton

**Task**: AZ-3742_test_harness_skeleton
**Name**: Test-harness skeleton
**Description**: The black-box harness entry command with profiles and reset, the test compose file with the canonical service and network names, the Python consumer with the CSV reporter and the NOT-RUN rule, and the `e2e-ctl` stub that refuses every action
**Complexity**: 5 points
**Dependencies**: AZ-3741_docker_compose_skeletons, AZ-3652_initial_structure
**Component**: Bootstrap (black-box harness; later owned by E-31)
**Tracker**: AZ-3742
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c08

## Problem

Every pipeline task and the 249 system scenarios (E-31) run through one harness. If the report format, the NOT-RUN rule and the network isolation of the consumer are not fixed first, each epic invents its own runner and a test that did not run can look like a pass. Until the API can serve (E-22), the harness must report honestly which parts of the stack are not there.

## Outcome

- One entry command runs a profile, writes the CSV report with the documented columns and exits non-zero on any FAIL.
- A test that cannot run is reported `SKIP` with an error message starting `NOT-RUN(<code>):`, never PASS, and the summary counts NOT-RUN per code.
- The consumer can reach the SUT only through the client side; it has no route to the backend or proxy networks.
- The `e2e-ctl` stub exists with its bearer token and audit log, and answers a typed `not_implemented` error for every action.

## Scope

### Included
- The entry command: `--profile <name>` for the documented profile names (`core`, `web`, `desktop`, `android-emu`, `perf`, `lan`, `gpu`), `reset`, verbose output; an unknown profile is refused; a documented profile with no services yet ends with a non-zero exit and a "not available yet" message, never a pass.
- The test compose file (project `sunnycloud-e2e`): networks and service names of `tests/environment.md` using the canonical names (`proxy`, `backend`, service `worker`; test-only `client-net`, `edge-net`, `ctl-net` internal); images by digest; per-run generated secrets for the services it starts.
- In this skeleton the `core` profile starts the database (with the role init script of E-00/c11) and the consumer only; the SUT, mock and shaper services join the profile in the epics that make them healthy (E-22, E-20, E-19, E-31).
- The consumer: Python 3.13, `pytest -v --tb=short -p e2e_report`, the reporter plugin writing `e2e-results/report.csv` (columns Test ID, Test Name, Execution Time (ms), Result, Error Message), `metrics.json`, `not-run.csv`, and the end-of-run summary with PASS, FAIL and NOT-RUN counts per code; a marker that turns a test into `NOT-RUN(<code>)`.
- Harness self-tests: the reporter writes one row per test; a NOT-RUN test produces the prefixed SKIP; the consumer cannot reach the database port.
- `reset`: restores the database volume to the golden snapshot taken after `up`.
- The `e2e-ctl` stub: host process bound to the ctl-net gateway, bearer token, one audit line per call in `ctl-audit.log`, typed `not_implemented` for every action of the Fault injectors table.

### Excluded
- Mocks (`mock-idp`, update server), the WAN shaper, data sets, the XFS blob-volume tooling, browsers, desktop and device drivers, lab runner, report merge (E-31).
- Any real `e2e-ctl` action (E-31).
- CI wiring of the `core` job (E-31, after E-21 and E-22).

## Acceptance Criteria

**AC-1: The core profile reports on the skeleton (E-00 AC 7)**
Given the skeleton stack and the harness self-tests
When the `core` profile runs
Then `e2e-results/report.csv` exists with exactly the five documented columns, one row per test, 0 FAIL, and the command exits 0

**AC-2: Not run is never a pass**
Given a test marked as needing a service the profile does not start
When the profile runs
Then its row has Result `SKIP` and an Error Message starting `NOT-RUN(`, it is listed in `not-run.csv`, and the summary prints the NOT-RUN count for its code

**AC-3: Any FAIL fails the run**
Given a seeded failing self-test
When the profile runs
Then its row has Result `FAIL` with a non-empty Error Message and the command exits non-zero

**AC-4: Consumer isolation**
Given the running `core` profile
When the consumer tries to open a TCP connection to the database port
Then the connection fails (the consumer has no route to `backend`)

**AC-5: e2e-ctl cannot be used to pass a test**
Given the `e2e-ctl` stub
When any documented action is called with the bearer token, and once without it
Then every authenticated call returns the typed `not_implemented` error and writes one audit line, and the unauthenticated call is refused and also audited

**AC-6: Verbose and resettable**
Given the harness files
When they are searched for quiet flags and `reset` runs after a test wrote rows to the database
Then the search finds no `pytest -q`, `--quiet` or `cargo test -q`, and `reset` returns the database to the snapshot in under 60 s

**AC-7: Profiles are explicit**
Given an unknown profile name and the documented `web` profile
When the entry command is run with each
Then the unknown name exits non-zero with the usage text, and `web` exits non-zero with "not available yet"

## Non-Functional Requirements

**Performance**
- The skeleton `core` run (start, tests, stop) finishes in under 5 minutes.

**Reliability**
- Every step prints output; logs and `e2e-results/` stay gitignored.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Reporter plugin on a synthetic pass, fail and skip | Rows with the five columns and correct results |
| AC-2 | NOT-RUN marker | SKIP row with `NOT-RUN(<code>):` prefix and entry in `not-run.csv` |
| AC-5 | e2e-ctl stub request handling | `not_implemented` for every action; 401-style refusal without the token; audit line each time |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Skeleton stack | `core` profile run | report.csv valid, 0 FAIL, exit 0 | Performance |
| AC-3 | Seeded failing self-test | `core` profile run | FAIL row, exit non-zero | — |
| AC-4 | Running stack | TCP connect from consumer to db | Fails | — |
| AC-6 | Rows written, then `reset` | Snapshot restore | Restored in < 60 s | Performance |
| AC-7 | Unknown and `web` profiles | Entry command | Both exit non-zero with the stated messages | — |

## Constraints

- Report format, NOT-RUN rule, isolation and timeouts follow `tests/environment.md` (Reporting, Networks, Isolation); no internal SUT imports in the consumer.
- Never quiet test runs (owner rule; `tests/environment.md`).

## Risks & Mitigation

**Risk 1: A skeleton that looks green hides missing services**
- *Risk*: a run that starts only the database reports 0 FAIL while most of the stack is absent.
- *Mitigation*: the summary prints the NOT-RUN counts, and the self-tests are the only tests the skeleton runs; component and pipeline tests that need the SUT are NOT-RUN until their services join the profile.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `tests/environment.md` names `worker-supervisor`, `proxy-net`, `backend-net`; canonical-names and the operator install use `worker`, `proxy`, `backend` ("test compose uses the same service names"). This task follows canonical-names; `environment.md` needs a doc fix. | Test owner (E-31) | open | Low |
| `tests/environment.md` defines NOT-RUN codes for hardware classes only; no code exists for "service not available in this build yet". The code must be chosen with the test owner. | Test owner (E-31) | open | Medium |
| The golden snapshot also covers the XFS blob image (`cp --reflink`), whose tooling is E-31's; the skeleton `reset` covers the database volume only. | Test owner (E-31) | accepted-risk | Low |
| This task meets rule 10 (many pipeline tasks depend on it) but has no contract folder in this batch's ownership; the existing interface document is used instead (see Contract). | Decompose parent | open | Low |

## Contract

This task has no new contract file. Its interface (entry command, profiles, report columns, NOT-RUN rule, `e2e-ctl` actions) is `_docs/02_document/tests/environment.md` § Test Execution, § Reporting and § Fault injectors; consumers read that document.
