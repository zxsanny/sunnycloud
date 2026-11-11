# Watchdog test trigger (G-10-4) and HLG poster SSIM record (G-10-6)

**Task**: AZ-3851_decoder_watchdog_hook_ssim_spike
**Name**: Decoder watchdog-hook and HLG SSIM spike
**Description**: Choose a test-only way to freeze only the runner's main loop so the self-abort watchdog can be tested, and measure the SSIM of tone-mapped HLG poster frames so the owner can set the open tolerance
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure; notes: E-31/c21 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 10_decoder
**Tracker**: AZ-3851
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c03

## Problem

IT-10-18 is BLOCKED: `SIGSTOP` stops every thread, including the watchdog, so nothing can make only the main loop hang (G-10-4). The HLG part of IT-10-7 is BLOCKED because the poster-frame SSIM tolerance is an open owner item (G-10-6, canonical-names "poster-frame SSIM tolerance"); without measured values the owner has nothing to decide on.

## Outcome

- A recorded test-only trigger that blocks the main loop while the watchdog thread runs, absent from release builds, which E-19/c07b implements and E-19/c15 uses for IT-10-18.
- Measured SSIM values of HLG poster frames, handed to the owner as the input for the tolerance; IT-10-7 keeps recording the value only until the owner answers.

## Scope

### Included
- Compare at most three trigger options (for example a feature-gated request, a feature-gated stub marker, a test-only signal handler) for: blocks only the main loop, needs no change to release code paths, can be proven absent from the release binary; record the choice in 10 §3 and `tests/spec-gaps.md` (G-10-4 moves to "resolved at decompose").
- Render poster frames of DS-06 base at the S5 candidate chain, compute SSIM against the harness SDR reference, record the values per frame and the method.
- Add the measured values to the owner question in the open owner items (the Step 4 sufficiency list).

### Excluded
- Implementing the trigger (E-19/c07b) and the poster path (E-19/c10).
- Setting the tolerance (owner decision).

## Acceptance Criteria

**AC-1: Trigger chosen (G-10-4)**
Given the trigger options
When the spike closes
Then 10 §3 names one trigger, how it is excluded from release builds, and the check that proves the release binary lacks it, and G-10-4 points to E-19/c07b and IT-10-18

**AC-2: HLG SSIM measured (G-10-6)**
Given DS-06 base and the S5 candidate tone-map chain
When poster frames at 1 s and at 10% of the duration are rendered and compared with the harness SDR reference
Then the SSIM values and the method are recorded in `tests/spec-gaps.md` under G-10-6

**AC-3: Owner question raised, test stays record-only**
Given the AC-2 values
When the spike closes
Then the open owner item "poster-frame SSIM tolerance" carries the values, and IT-10-7 states that its HLG part records SSIM without a pass criterion until the owner answers

## Non-Functional Requirements

**Reliability**
- The trigger must not exist in the release image (same rule as the `test-clock` feature).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-3 | Spike record review (10 §3, `tests/spec-gaps.md`, open owner items) | Trigger named with its release check; owner question carries the values |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | DS-06 base | SSIM of HLG posters vs reference | Values recorded, no pass/fail | IT-10-7, AC-10 |

## Constraints

- The spike does not choose a tolerance; G-10-6 is an owner item.

## Risks & Mitigation

**Risk 1: The trigger changes release behaviour**
- *Risk*: a hook in the main loop slows or alters the release path.
- *Mitigation*: AC-1 requires compile-time exclusion and a binary check.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-10-6: HLG poster-frame SSIM tolerance is an open owner item; IT-10-7 (HLG part) and IT-10-10 stay record-only until it is set | Project owner | open | Medium |
