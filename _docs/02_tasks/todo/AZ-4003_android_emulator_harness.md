# Android emulator test harness (profile `android-emu`)

**Task**: AZ-4003_android_emulator_harness
**Name**: Android emulator harness
**Description**: The `android-emu` test profile: an API 34+ emulator (plus API 30 and 33 images) with Appium and `adb` media push, a rooted kill loop, memory and traffic sampling, and the hookup to the server's 15-minute token fixture and test clock, all against the real server stack
**Complexity**: 5 points
**Dependencies**: AZ-4002_native_library_ci, AZ-3831_internal_router_test_clock, AZ-3742_test_harness_skeleton
**Component**: 17 android_app (component test tooling)
**Tracker**: AZ-4003
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c19

## Dependencies

### Document Dependencies
- `components/17_android_app/tests.md` (tiers, Test Data Management: setup, teardown, isolation); `tests/environment.md` (EMU tier).
- Test clock routes `POST /test/clock` and the internal clock offset (E-21/c15); canonical-names § Runtime (test image only).

## Problem

Almost every E-27 component test runs a release-signed build on an emulator against the real server container with real PostgreSQL (17 tests tiers). Those tests need the same tooling: push photos into MediaStore, drive the app with Appium, kill the process with root during a run, sample memory and network traffic, run the server's token fixture with the test clock, and reset everything between tests. Built once here, the tests of E-27/c08 to c17, the pipelines AZ-3672 and AZ-3673 and E-31's mobile scenarios share it.

## Outcome

- `tests/run.sh --profile android-emu` starts the emulator (KVM, userdebug image for root), Appium UiAutomator2 and the real server stack, installs the release-signed build from E-27/c18, and runs the selected tests.
- Helpers: media push with `adb push` and a media scan (DS-21 fixtures, scalable to 50,000), WorkManager test-driver time control, a rooted kill loop at random moments, `dumpsys meminfo` sampling, a traffic capture on the emulator network, a proxy byte log, account setup through an invite token and teardown through `e2e-ctl`.
- The token fixture: server test clock for 15-minute access tokens and rotating refresh tokens (NFT-RES-33 fixture).
- Extra emulator images API 30 and 33.

## Scope

### Included
- Profile wiring in the E-00 harness skeleton, images, Appium server, helpers above, snapshots and reset.
- A self-test of every helper with the app from E-27/c18 (no backup logic needed).
- Group `X` for kill loops and test-clock runs.

### Excluded
- The component tests themselves (their owning tasks); phone lab runs (E-31/c45).

## Acceptance Criteria

**AC-1: Profile starts and tears down**
Given a KVM host
When `tests/run.sh --profile android-emu` runs a trivial test and ends
Then the emulator, Appium and the real server stack start, the release-signed build is installed, the test runs with verbose output, and teardown uninstalls the app, clears test media, purges the test account and resets the snapshot

**AC-2: Media push**
Given the DS-21 fixture
When 100 and then 50,000 assets are pushed
Then MediaStore reports exactly that many new rows after the scan

**AC-3: Rooted kill loop**
Given the app running
When the kill loop kills the app process 10 times at random moments
Then each kill is logged with its time and the process restarts each time

**AC-4: Samplers**
Given a running app
When memory sampling every 1 s and 10 s, the traffic capture and the proxy byte log run for one minute
Then each produces its record (memory per sample, captured packets with cleartext flagged, bytes per route)

**AC-5: Token fixture and clock**
Given the server test image
When a test sets the clock offset beyond 15 minutes through the test-clock route
Then the next authenticated request of the app gets a 401 and a refresh, and the release server image has no test-clock route (404)

**AC-6: Images API 30 and 33**
Given the profile with `--image 30` or `--image 33`
When the trivial test runs
Then it passes on each image

## Non-Functional Requirements

**Reliability**
- One emulator snapshot, one account and one device label per test; no shared mutable state between tests.

**Compatibility**
- Same service names as the production compose files (canonical-names § Runtime).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Kill-loop timing generator with a seed | Reproducible kill times |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | KVM host | Profile run and teardown | Stack up; clean teardown | |
| AC-2 | DS-21 | Push 100 and 50,000 | Exact MediaStore counts | IT-17-13 |
| AC-3 | Running app | 10 kills | Logged and restarted | IT-17-13 |
| AC-4 | Running app | Samplers for 1 min | Records produced | PT-17-2, IT-17-6 |
| AC-5 | Test image | Clock beyond 15 min | 401 then refresh; release image 404 | NFT-RES-33 |

## Constraints

- Fakes are not allowed in this tier: real server, real PostgreSQL, real MediaStore; only the identity provider is mocked (mock-idp).
- Test commands never run in quiet mode.

## Risks & Mitigation

**Risk 1: Emulator flakiness**
- *Risk*: emulators hang on CI hosts.
- *Mitigation*: every run has a watcher with progress lines; a hung emulator fails the run with its log instead of waiting.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The CI host for KVM emulators is not named in the deployment docs | Owner / E-00 | open | Low |
