# Integration suite IT-11-1..20 and crate-graph tests

**Task**: AZ-3835_integration_suite_crate_graph
**Name**: HTTP API integration suite and crate graph
**Description**: Run the IT-11-1..20 cases as one CI job with their time budgets and a missing-case check, add the real-stack (Mode R) parts of IT-11-9 and IT-11-17, extend the crate-graph test with 11's rules, and enforce the 90% coverage floor on CSRF, auth gating, error mapping and header mapping
**Complexity**: 3 points
**Dependencies**: AZ-3831_internal_router_test_clock, AZ-3832_ops_endpoints_readiness, AZ-3833_graceful_shutdown, AZ-3834_csrf_idor_matrices, AZ-3761_ci_skeleton, AZ-3791_serve_start_stop; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); E-31/c07 (test epic E-31, decomposed at greenfield Step 9); E-31/c09 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 11 http_api
**Tracker**: AZ-3835
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c19

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/http_server.md` — listeners and builds with and without `test-clock` (E-21/c03).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — worker and test-clock placement (E-21/c11).
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — `/readyz` body (E-09/c01).

## Problem

Each E-21 task writes the IT-11 case of its own layer against the stub-services server (Mode S). Three things are still missing: proof on the real stack that worker and test routes are where they should be in both images and that readiness follows a real database outage and a pending migration (Mode R parts of IT-11-9 and IT-11-17); one CI job that runs every case within its budget and notices a case that was never written; and the architectural guard that keeps business logic and SQL out of the delivery layer (ADR-015).

## Outcome

- One CI job runs IT-11-1 to IT-11-20, reports each case id with its result and duration, and fails when a case id of `components/11_http_api/tests.md` has no test or exceeds its max execution time.
- Mode R runs of IT-11-9 (test and release images) and IT-11-17 (database outage, pending migration).
- The crate-graph test fails when 11 imports a component crate, `sc_db` or `sqlx`, or when a component crate imports 11.
- Coverage gate: at least 90% on the CSRF layer, auth extraction, error mapper and content header mapper; at least 75% on the rest of the crate.

## Scope

### Included
- The suite runner and its missing-case and budget checks.
- IT-11-9 Mode R: the test image and the release image, requests to `:8080`, `:8081`, `:9090`, `:9091` and through Caddy.
- IT-11-17 Mode R: stop and restart `db`; an upgrade-interrupted state with a pending migration.
- 11's rows in the E-00 crate-graph test (AZ-3652) and the coverage gate in CI (E-00/c09).

### Excluded
- The Mode S assertions of each case (written by the owning E-21 task: c03, c04, c05, c06, c07, c08, c09, c10, c11, c13, c14, c15, c16, c17, c18).
- PT, ST and AT cases (E-21/c20); system scenarios (E-31); pipelines through real components (AZ-3654 to AZ-3667).

## Acceptance Criteria

**AC-1: Suite completeness and budgets**
Given the case list IT-11-1 to IT-11-20 of `components/11_http_api/tests.md`
When the CI job runs
Then it reports every case id with result and duration, fails if any id has no test, and fails a case that exceeds its max execution time

**AC-2: Listener placement on the real images (AC-79, FT-N-35)**
Given the Compose test stack with the test image, then with the release image
When `POST /api/v1/worker-jobs/claims`, `GET .../{id}/input` and `PUT .../status` are sent to `:8080` and through Caddy, the same with user credentials and the worker bearer to `:8081`, `/healthz`, `/readyz`, `/metrics` to `:9090` and `:8080`, and the clock endpoints to `:9091` and `:8081`
Then worker routes are 404 on `:8080` and through Caddy, 403 for user credentials and reachable with the worker bearer on `:8081`; ops paths answer only on `:9090`; the test image serves both clock endpoints; the release image refuses `:9091` and answers 404 for `/internal/v1/test/clock-offset` and `/test/clock`

**AC-3: Readiness follows the real database (AC-22, AC-65)**
Given the Compose test stack
When `db` is stopped, then restarted, and later the stack is left with a pending migration
Then `/readyz` is 503 during the outage and 200 within 60 s of the database accepting connections, `/healthz` stays 200 throughout, and the pending migration gives 503 naming migrations

**AC-4: Delivery layer stays thin (ADR-015)**
Given the workspace
When the crate-graph test runs, and again with a planted import of a component crate and of `sc_db` into 11
Then the clean run passes and each planted import fails it, naming the edge

**AC-5: Coverage floor**
Given the full test run with coverage
When the gate reads coverage per area
Then the CSRF layer, auth extraction, error mapper and content header mapper each reach at least 90% and the crate at least 75%, and a drop below either fails CI

## Non-Functional Requirements

**Reliability**
- Mode R cases create their own accounts or run on a restored golden stack (group X for the database outage), as `tests.md` states.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Runner over a fixture list with one missing id | Fails naming it |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | CI | Suite run | Every id reported; missing id fails | |
| AC-2 | Mode R, both images | Listeners and route placement (IT-11-9, Mode R) | As listed | AC-79, FT-N-35, NFT-SEC-50 |
| AC-3 | Mode R, group X | Readiness under outage (IT-11-17, Mode R) | 503, then 200 within 60 s; migration named | AC-22, AC-65, NFT-RES-20, FT-N-39 |
| AC-4 | Workspace | Crate graph | Planted edges fail | |
| AC-5 | CI coverage | Coverage gate | 90% and 75% floors | quality thresholds |

## Constraints

- Mode R uses the real `sunnycloud serve` binary, real PostgreSQL 18 and the release Caddyfile; no component is faked in Mode R.

## Risks & Mitigation

**Risk 1: Duplicate tests with the pipelines**
- *Risk*: AZ-3660 also probes worker routes through Caddy.
- *Mitigation*: this task checks placement on both images and every listener; AZ-3660 checks the media flow end to end.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Re-estimated from the planned 5 to 3 points: the Mode S assertions of each IT-11 case are written by the task that owns that layer (batch convention), so this task keeps the runner, the Mode R parts, the crate graph and the coverage gate | E-21 owner | open | Low |
| Per-area coverage measurement needs a tool that reports by module; the tool is chosen with E-00/c09's coverage gate | E-00, E-21 owners | open | Low |
