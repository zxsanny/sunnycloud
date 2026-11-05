# [v1.1] macOS placeholder (SF_DATALESS) and TCC behaviour (spike)

**Task**: AZ-3921_macos_placeholder_tcc_spike
**Name**: macOS placeholder and TCC spike
**Description**: Record how evicted iCloud files are marked (`SF_DATALESS`) and which calls hydrate them, and how macOS privacy controls (TCC) deny a background agent access to protected folders, so the macOS platform layer and its permission probe are built on measured behaviour
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3921
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c23

## Problem

AC-32 forbids hydrating evicted placeholders; on macOS they carry the dataless flag, but some metadata calls may still trigger a download. AC-67 needs the daemon to notice within 10 s that macOS denies a folder and to notice the grant; how a LaunchAgent sees a TCC denial (error kind, prompt, silent empty listing) is unverified.

## Outcome

- A written record in 14 §5 (Placeholders, macOS) and §15: which `stat`, `getattrlist`, `open` and directory-listing calls are safe on a dataless file and which hydrate it; the error or behaviour when TCC denies `~/Documents`, `~/Desktop` and `~/Downloads` to a LaunchAgent; whether a prompt appears; how a grant becomes visible without restart.
- The probe rule (what call, how often within the 10 s budget) for E-24/c21.
- Unblocks E-24/c21.

## Scope

### Included
- Current macOS on a lab Mac; iCloud Drive with evicted files; a LaunchAgent test binary.

### Excluded
- The production macOS layer (E-24/c21).

## Acceptance Criteria

**AC-1: Safe calls on dataless files recorded**
Given evicted iCloud files
When the spike calls `stat`, `getattrlist`, directory listing and `open` on them and watches the iCloud download activity
Then 14 §5 lists which calls are safe (0 bytes hydrated) and which hydrate

**AC-2: TCC denial behaviour recorded**
Given a LaunchAgent test binary without access to `~/Documents`
When it lists and opens files there, and the user then grants access in System Settings
Then 14 §15 records the error or behaviour seen, whether a prompt appears, and how long after the grant access works

**AC-3: Probe rule written**
Given AC-1 and AC-2
When the spike ends
Then E-24/c21's probe call and period fit the 10 s budget of AC-67, and IT-14-24's expected result names the observable

## Non-Functional Requirements

**Compatibility**
- Results stated per macOS version; absent hardware `NOT-RUN(HW-DEVICE)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: measurement outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Lab Mac, iCloud, LaunchAgent test binary | Calls and grant | Recorded | AC-32, AC-67, FT-P-75 |

## Constraints

- v1.1 only; label `release:v1.1`. The test binary is throwaway.

## Risks & Mitigation

**Risk 1: No lab Mac**
- *Risk*: the spike cannot run.
- *Mitigation*: it stays open and blocks E-24/c21; v1.0 is unaffected.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Mac hardware is an owner item (G11) | Owner | open | Low |
