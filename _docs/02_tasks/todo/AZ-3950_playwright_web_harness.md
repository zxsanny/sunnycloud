# Playwright `web` profile harness, Safari lab runner and heap sampler

**Task**: AZ-3950_playwright_web_harness
**Name**: Web test harness
**Description**: The web app's Playwright setup on the `web` test profile: real stack behind the release Caddyfile, per-test accounts and contexts, fault-proxy hooks, network and CSP-violation logs, a heap sampler for three engines and the Safari runner on the owner Mac
**Complexity**: 3 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3742_test_harness_skeleton, AZ-3652_initial_structure; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); E-31/c13 (test epic E-31, decomposed at greenfield Step 9); E-31/c15 (test epic E-31, decomposed at greenfield Step 9); E-31/c16 (test epic E-31, decomposed at greenfield Step 9), AZ-4021_release_caddyfile_security_headers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3950
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c19a

## Problem

Every Playwright test in this epic (IT-16-2 to IT-16-19, ST-16-1 to ST-16-4, PT-16-1 to PT-16-5) runs against the real server with real PostgreSQL behind the release Caddyfile, with the fault proxy, fresh browser contexts and heap numbers per engine (16 tests.md "Setup", environment.md). Safari needs `safaridriver` on the owner Mac; Playwright WebKit is not a substitute.

Split note: epic row 19 (`Playwright web profile harness, Safari lab runner, 50 GiB heap sampling (PT-16-1..4)`, 5 points) mixes an early enabler, needed by every Playwright test of this epic, with late 50 GiB measurements, which need the finished upload and download engines. The two cannot be one task in dependency order; together they are about 6 points. This task keeps the harness; E-26/c19b runs the 50 GiB measurements.

## Outcome

- `tests/run.sh --profile web` runs the web app's Playwright suites in Chromium and Firefox (WebKit for inline checks) against the real stack.
- Tests get a fresh account and browser context each, fault injection, a network log, a CSP-violation collector, a DOM observer and heap samples.
- Safari runs on the owner Mac through the same tests; without the Mac the result is `NOT-RUN(HW-DEVICE)`, never a pass.

## Scope

### Included
- Playwright projects and configuration inside the web project; the `web` profile entry using E-31's compose file, `browsers` container, mock-idp and fault proxy.
- Fixtures: account per test by invite plus mock-idp sign-in, fresh context per test, teardown that purges the account through `e2e-ctl` and deletes downloaded files.
- Helpers: fault rules (status, `Retry-After`, drop after request), network log, CSP-violation collector, DOM observer for inline-render checks, heap sampler (Chromium CDP metrics, Firefox memory report, Safari `vmmap` footprint labelled weak).
- Safari runner: `safaridriver` on H-MAC with result mapping to the shared report.

### Excluded
- E-31's containers, compose file, generators and CI wiring of the nightly `web` job (E-31/c05, c16, c17, c19); the 50 GiB runs (E-26/c19b).

## Acceptance Criteria

**AC-1: The profile runs a smoke suite**
Given the `web` profile
When it is started and the smoke suite runs (sign in through mock-idp, list the root folder) in Chromium and Firefox
Then both pass in fresh contexts and the run exits 0; with the server not started the run exits non-zero

**AC-2: Fault injection reaches the page**
Given a fault rule that answers one route with 507, and a rule that drops the connection after a request is sent
When a test triggers each route
Then the page receives the 507 problem body, and the second request is seen by the server but its response never reaches the page

**AC-3: Heap sampler per engine**
Given a probe page that allocates and then releases 64 MB
When the sampler runs every 5 s in Chromium and Firefox, and in Safari on the Mac
Then each series rises by at least 64 MB and falls back, and the Safari series is labelled weak evidence

**AC-4: Safari runner reports honestly**
Given the owner Mac, and in a second run no Mac
When the smoke suite is started for Safari
Then it runs through `safaridriver` and reports pass or fail; without the Mac the report says `NOT-RUN(HW-DEVICE)` and never pass

**AC-5: Isolation and teardown**
Given two tests run in a row
When the second starts
Then it has another account and a fresh context, and the first test's account and downloaded files are gone

**AC-6: Logs catch seeded faults**
Given a probe page with a seeded CSP violation, a request to a third-party host and an injected `<iframe>`
When a test runs it
Then the collector reports the violation, the network log lists the foreign host, and the DOM observer reports the frame

## Non-Functional Requirements

**Reliability**
- Long runs print a heartbeat; no new output for 2 minutes is reported STALLED and aborted (environment.md timeouts).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Result mapping for pass, fail and missing host | `NOT-RUN(HW-DEVICE)` only for a missing host |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-5 | `web` profile, mock-idp | Smoke suite twice | Pass; isolated accounts | |
| AC-2 | Fault proxy rules | Two probe calls | 507 seen; dropped answer | |
| AC-3 | Probe page | Heap sampler | Rise and fall per engine | PT-16-1, PT-16-2 |
| AC-4 | With and without H-MAC | Safari smoke | Pass or `NOT-RUN(HW-DEVICE)` | |
| AC-6 | Seeded probe page | Collectors | All three reported | IT-16-2, IT-16-9 |

## Constraints

- No fake API at the browser boundary; only the fault proxy changes answers (16 tests.md "Isolation").
- The Caddyfile in the stack is the release Caddyfile (containerization.md).

## Risks & Mitigation

**Risk 1: Heap numbers differ between engines' tools**
- *Risk*: the three sources measure different things.
- *Mitigation*: each sample names its source; limits are judged per engine as AC-50 and AC-51 state.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Safari runs depend on the owner Mac (H-MAC) being in the inventory (E-31/c01) | Owner (hardware) | open | Low |
