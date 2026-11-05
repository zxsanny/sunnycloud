# iOS test harness: Appium XCUITest on iPhone and simulator against the real stack

**Task**: AZ-3983_ios_xcuitest_harness
**Name**: iOS XCUITest harness
**Description**: The IT-IOS tier: Appium XCUITest runs on DEV-IPHONE and the simulator against the real server container with real PostgreSQL, with photo import, debug background-task triggers, invite-token accounts, an Apple-shaped mock provider and honest `NOT-RUN(H-IOS)` reporting
**Complexity**: 3 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3742_test_harness_skeleton
**Component**: 18 ios_app [v1.1] (test harness)
**Tracker**: AZ-3983
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c11a

## Dependencies

### Document Dependencies
- `_docs/02_document/components/18_ios_app/tests.md` — tiers, Test Data Management (setup, teardown, isolation).
- `_docs/02_document/tests/environment.md` — where each tier runs; the `lan` profile.

## Problem

Every IT-18 test of E-28/c04 to c10 runs as Appium XCUITest on a real iPhone (H-IOS, DEV-IPHONE) or, for non-background flows, the simulator, against the real server container. That needs a harness: a server profile with a publicly trusted certificate (iOS ignores user CAs), photos imported into the device library, a way to trigger background tasks without waiting on the clock, test accounts and teardown. The Mac and phone are owner hardware, so a missing device must never be reported as a pass.

**Split note**: the epic row c11 ("Appium XCUITest suite, memory profiling and AT-18-1..3 (PT-18-1)", 5 points) is split in two. The harness (this task, c11a) is needed by the IT-18 tests of c04 to c10, while the memory and acceptance tests (c11b) need those tasks done; harness, profiling and three acceptance tests together exceed 5 points.

## Outcome

- One command runs a chosen IT-18 test against the real stack on the simulator or on DEV-IPHONE, with verbose output and a per-test result.
- Tests can import fixture photos, trigger `BGProcessingTask` and `BGAppRefreshTask` with the Xcode debug command, and sign in with an invite-token account through an Apple-shaped mock provider.
- A run without the Mac or the phone records `NOT-RUN(H-IOS)` for the affected tests, never PASS.
- Teardown deletes the app and test photos and purges the account through `e2e-ctl`.

## Scope

### Included
- Appium XCUITest driver setup for DEV-IPHONE and the simulator; device selection per test (background tests need the device).
- Server side: the real server container with real PostgreSQL in the `lan` profile with a publicly trusted certificate; an Apple-shaped provider from `mock-idp`; a proxy log for byte counting.
- Fixtures: DS-21 photos imported into the device or simulator library (`simctl addmedia` or Xcode), the NFT-RES-33 token fixture hookup with the test clock.
- Account per test via invite token, one device label per test, teardown through `e2e-ctl`.
- Result reporting with `NOT-RUN(H-IOS)` and a leftover note when the hardware is absent.

### Excluded
- The tests themselves (owned by E-28/c04 to c10 and c11b) and the system scenarios (E-31).
- Signing and distribution (owner).

## Acceptance Criteria

**AC-1: A sample test runs on the simulator**
Given the owner Mac, the simulator and the stack in the `lan` profile
When the harness runs a sample test that imports one photo, signs in through the mock provider and reads the app status
Then the test passes and the output shows each step verbosely

**AC-2: Background task trigger without waiting**
Given DEV-IPHONE with a debug build
When a test triggers `BGProcessingTask` through the debug command
Then the app's background handler starts within the test's step timeout, with no wall-clock wait for the system to schedule it

**AC-3: Honest result without hardware**
Given no Mac or no phone available
When the IT-IOS tier is started
Then every affected test is reported `NOT-RUN(H-IOS)`, none as PASS, and a note is written to `_docs/_process_leftovers/`

**AC-4: Isolation and teardown**
Given two tests run one after the other
When each finishes
Then each used its own account and device label, and teardown removed the app, the imported photos and the account (purged through `e2e-ctl`, not the deletion API)

## Non-Functional Requirements

**Reliability**
- No quiet test flags; every failing step prints its context so a failure is diagnosable without a re-run.

**Compatibility**
- Integration runs always use the real server container and real PostgreSQL; fakes are limited to Rust unit tests at the client trait boundary (18 tests.md).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Result mapping when the device probe fails | `NOT-RUN(H-IOS)`, never PASS |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Simulator, `lan` stack | Sample test | Passes; verbose output | |
| AC-2 | DEV-IPHONE, debug build | Debug BG trigger | Handler starts; no clock wait | IT-18-4 |
| AC-3 | No hardware | Tier start | `NOT-RUN(H-IOS)`; leftover note | |
| AC-4 | Two tests | Teardown | Own account and label; all removed | |

## Constraints

- Test tiers run on their designated host; a workstation run is never reported as an H-IOS result.
- Background triggers use the debug command, never wall-clock waiting (18 tests.md, Isolation).

## Risks & Mitigation

**Risk 1: Owner hardware not available**
- *Risk*: the Mac or iPhone is missing for long periods.
- *Mitigation*: AC-3 makes the gap visible in every report instead of hiding it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Owner Mac with Xcode, DEV-IPHONE and an iCloud account are owner hardware not yet named (performance G1, G11) | Owner | open | Medium |
| The `lan` profile needs a publicly trusted certificate for a lab host; who provides the name and certificate is not stated | Owner | open | Low |
