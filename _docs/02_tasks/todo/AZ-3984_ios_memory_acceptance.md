# iOS memory profiling and acceptance tests

**Task**: AZ-3984_ios_memory_acceptance
**Name**: iOS memory and acceptance tests
**Description**: Measure the uploader's memory footprint on iOS during a first sync and a background window (PT-18-1), and run the three iOS acceptance tests: foreground backlog, provider rule and limited access
**Complexity**: 3 points
**Dependencies**: AZ-3983_ios_xcuitest_harness, AZ-3976_ios_photokit_asset_source, AZ-3978_ios_auth_session_provider_rule, AZ-3979_ios_background_scheduling, AZ-3980_ios_flutter_ui
**Component**: 18 ios_app [v1.1]
**Tracker**: AZ-3984
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c11b

## Dependencies

### Document Dependencies
- `_docs/02_document/components/18_ios_app/tests.md` — PT-18-1, AT-18-1, AT-18-2, AT-18-3.
- `_docs/02_document/tests/resource-limit-tests.md` — NFT-RES-LIM-03 (iOS leg).

## Problem

The component acceptance of 18 has to be shown on a real iPhone: the uploader stays within the mobile memory bound (AC-52), a user can bring the library up to date by keeping the app open (AC-9 iOS), the sign-in screen follows the server's Apple setting (AC-19), and limited access shows the banner and backs up new picks (AC-53). Each part has a component test; this task runs the acceptance and memory tests on the assembled app.

**Split note**: second half of epic row c11 (see E-28/c11a for the reason).

## Outcome

- PT-18-1 recorded: memory footprint sampled every 5 s with Instruments during a foreground first sync of 5,000 assets and one granted background window.
- AT-18-1, AT-18-2 and AT-18-3 pass on DEV-IPHONE, or are recorded `NOT-RUN(H-IOS)` without the hardware.

## Scope

### Included
- PT-18-1 with Instruments footprint sampling.
- AT-18-1 (500 pending assets reach 500 of 500 in the foreground), AT-18-2 (two servers, with and without Sign in with Apple), AT-18-3 (limited access, 20 visible plus 10 picked).
- The test report with the measured footprint peak and each acceptance result.

### Excluded
- The harness (E-28/c11a) and component tests IT-18-1 to IT-18-10 (E-28/c04 to c10).
- The stricter memory ceiling of the iOS upload extension (deferred with the extension).
- System scenarios FT-P-31, FT-P-35, FT-P-63, FT-N-34 (E-31).

## Acceptance Criteria

**AC-1: Memory within the mobile bound (AC-52)**
Given a foreground first sync of 5,000 assets and one granted background window
When the footprint is sampled with Instruments every 5 s
Then the peak is recorded, the target is at most 120 MB, and the test fails above 150 MB

**AC-2: Foreground backlog (AC-9 iOS, AT-18-1)**
Given a signed-in app with 500 pending assets
When the app is opened and kept in the foreground
Then progress with an ETA appears, the count reaches 500 of 500, and the server holds 500 files in `Camera Uploads/<label>/`

**AC-3: Provider rule (AC-19, AT-18-2)**
Given two servers, one with Sign in with Apple enabled
When the app connects to each
Then the first offers Apple, Google and GitHub, and the second shows no Google or GitHub button

**AC-4: Limited access (AC-53, AT-18-3)**
Given "Selected Photos" authorization with 20 photos
When the status screen opens, the user taps the banner and picks 10 more, and the next run completes
Then the banner read `20 photos visible, tap to allow more` and the 10 picks are uploaded

## Non-Functional Requirements

**Performance**
- Footprint target 120 MB, failure above 150 MB (PT-18-1, NFT-RES-LIM-03).

**Reliability**
- Without DEV-IPHONE or the Mac every test here is `NOT-RUN(H-IOS)`, never PASS.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (acceptance and measurement task) | — |

## Blackbox Tests

On DEV-IPHONE (H-IOS) through the harness of E-28/c11a, real server stack.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 5,000 assets; one background window | Footprint every 5 s | Peak <= 120 MB target; fail > 150 MB | PT-18-1, NFT-RES-LIM-03 |
| AC-2 | 500 pending | Foreground run | 500 of 500; 500 server files | AT-18-1, FT-P-31 |
| AC-3 | Two servers | Sign-in screens | Apple+Google+GitHub; then none of Google/GitHub | AT-18-2, FT-P-63, FT-N-34 |
| AC-4 | Selected Photos, 20 | Banner, pick 10, next run | Exact banner; 10 uploaded | AT-18-3, FT-P-35 |

## Constraints

- Timing of background windows is recorded, never asserted (RESTRICT-27).

## Risks & Mitigation

**Risk 1: Footprint differs from Android's RSS**
- *Risk*: Instruments footprint and Android RSS/PSS are not the same measure.
- *Mitigation*: the report names the measure used; the 150 MB bound is applied to the footprint as PT-18-1 states.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Owner hardware (Mac, DEV-IPHONE, iCloud account) and the 5,000-asset library are not yet provisioned (performance G1, G11) | Owner | open | Medium |
| Memory measure definitions (RSS vs PSS vs footprint) and averaging windows are open in resource-limit G2-G4 | Implement / owner | open | Low |
| G-7: no time bound for the foreground catch-up; AC-2 asserts completion, not duration | Owner | open | Low |
