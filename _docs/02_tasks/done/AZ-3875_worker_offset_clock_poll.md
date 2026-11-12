# OffsetClock poll in the test build (feature test-clock)

**Task**: AZ-3875_worker_offset_clock_poll
**Name**: Worker offset clock poll
**Description**: In test builds only, poll the API's clock offset over the worker listener and apply it to the shared offset clock so the supervisor and the API agree on wall time; release builds contain neither the poll nor the clock
**Complexity**: 2 points
**Dependencies**: AZ-3874_worker_status_shutdown_main, AZ-3867_worker_spike_slots_clock_scratch, AZ-3869_worker_api_client, AZ-3765_sc_clock
**Component**: 09_worker_supervisor
**Tracker**: AZ-3875
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `OffsetClock::advance`, `offset` (feature `test-clock`), produced by E-03/c01.

## Problem

System tests advance the API's clock to pass retention, leases and image age in seconds (AC-30 style). The supervisor must see the same offset, or its status report (image age) disagrees with the API's clock. The poll must never exist in a release image, where it would let anyone shift time (FT-N-35).

## Outcome

- In the `-test` image the supervisor follows the API's offset; the effect fixed by E-20/c01 (G-09-4) is observable.
- In the release image there is no poll request and no offset clock symbol.

## Scope

### Included
- Feature `test-clock` only: poll `GET /internal/v1/test/clock-offset` on the worker listener with the worker bearer; apply the returned offset to the `sc_clock` offset clock; `main` uses that clock in test builds.
- The effect chosen by E-20/c01 for G-09-4 made observable.

### Excluded
- The API endpoints (E-21/c15) and the release-binary symbol check (E-03/c01, run in the CI `images` job).

## Acceptance Criteria

**AC-1: Offset followed (IT-09-18)**
Given the worker `-test` image and the API advanced by 3,600 s through the test control endpoint
When the supervisor polls
Then the poll uses the worker bearer, the supervisor's offset becomes 3,600 s, and the G-09-4 effect recorded by E-20/c01 shows it

**AC-2: Release build has no poll (IT-09-18, FT-N-35)**
Given the release worker image of the same version
When it runs for 30 s and its binary is checked
Then the API log shows 0 requests to the offset route and the binary has no offset clock symbol

## Non-Functional Requirements

**Security**
- The poll and the offset clock exist only with the `test-clock` feature (sc_clock invariant).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Applying a polled offset to the offset clock | Offset equals the API value |

## Blackbox Tests

Run in harness H2 (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Test and release images | IT-09-18 | Offset followed; release silent | FT-N-35, NFT-SEC-38 |

## Constraints

- No poll code path in release builds (compile-time exclusion, not a runtime flag).

## Risks & Mitigation

**Risk 1: The test image reaches production**
- *Risk*: an operator runs the `-test` tag.
- *Mitigation*: CI asserts the 404 of the clock routes on the pushed release digest (containerization "Release-versus-test rule").

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `sc_clock` offset only grows (`NegativeAdvance`), but the test setup resets the offset to 0 per test (09 tests "Setup procedure"); the supervisor cannot follow a reset without a restart | E-03 and E-31 owners | open | Medium |
| The poll interval and the behaviour when a poll fails are not stated, and no config key exists for the interval | E-20 owner, helper 01 | open | Low |
