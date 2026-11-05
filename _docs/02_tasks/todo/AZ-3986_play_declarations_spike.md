# Spike: Play Store declarations for photo access, original location and user-initiated transfers

**Task**: AZ-3986_play_declarations_spike
**Name**: Play declarations spike
**Description**: Record, with sources, whether the app's photo and video permissions, `ACCESS_MEDIA_LOCATION`, the battery-exemption request and the large-first-sync mechanisms are accepted by Play policy, and what the app must declare
**Complexity**: 2 points
**Dependencies**: None
**Component**: 17 android_app (release and permissions)
**Tracker**: AZ-3986
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c02

## Dependencies

### Document Dependencies
- `components/17_android_app/description.md` §5 (permission states, battery/OEM, large first sync), §15 risk 2; ADR-016 (Android in v1.0, minSdk 30); epic risks R18 and R32.

## Problem

Camera backup needs broad photo and video access, the original's location data (`ACCESS_MEDIA_LOCATION` with `setRequireOriginal`), a battery-exemption request, a user-initiated data-transfer job on Android 14 and newer, and a user-started `dataSync` foreground service on Android 11 to 13. Play restricts several of these and asks for declarations; Android 15 caps `dataSync` services at 6 hours per 24 hours. If any of them is refused late, the backup design (E-27/c09, c10, c11, c17) changes after it is built.

## Outcome

- A record in `components/17_android_app/description.md` §15 risk 2 and the Play declaration runbook notes (epic Definition of Done): per permission or mechanism, a verdict, the declaration text needed, the policy source and its date.
- A named alternative for anything refused, with the tasks it changes.
- Owner items listed with status (Play account, closed testing, pre-review).

## Scope

### Included
- Photo and video read permissions, the Android 14 "selected photos" permission, `ACCESS_MEDIA_LOCATION`.
- The battery-optimisation exemption request.
- The user-initiated data-transfer job (Android 14+) and the `dataSync` foreground service (Android 11 to 13), including the Android 15 time cap.
- No always-on foreground service for automatic backup (already excluded by 17 §5; the record confirms the policy reason).

### Excluded
- Building any of the mechanisms (E-27/c09, c10, c11, c17).
- Opening the Play account or submitting a build (owner, R18).

## Acceptance Criteria

**AC-1: A verdict per permission and mechanism**
Given the list in Scope
When the spike ends
Then each item has a verdict (allowed, allowed with a declaration, or refused), the declaration text if any, the policy source and the date it was read

**AC-2: Battery-exemption request**
Given that 17 §5 makes the app request the battery exemption
When Play's rule for that request is checked
Then the record states whether the direct request is allowed for a backup app, or that the app must send the user to the system settings screen, and E-27/c17 is named as the task that follows it

**AC-3: Alternatives for refusals**
Given any item with the verdict refused
When the spike ends
Then the record names the alternative (for example the sideload or F-Droid contingency of R18) and each E-27 task whose scope changes

**AC-4: Owner items**
Given the Play account, closed testing and pre-review are owner actions
When the spike ends
Then each is listed with its status, and a pre-review not yet done is marked as not run, never as accepted

## Non-Functional Requirements

**Compatibility**
- Applies to minSdk 30 up to the newest Android release at the time of the spike.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Item list | Record review | Every item has verdict, source, date | R18 |
| AC-4 | Owner items | Record review | Status per item; nothing marked accepted without evidence | R18 |

## Constraints

- Sources are Play policy pages and Android documentation; a verdict without a source is not accepted.

## Risks & Mitigation

**Risk 1: Policy changes after the record**
- *Risk*: Play policy changes between the spike and release.
- *Mitigation*: the record carries the read date; the release checklist re-reads it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Play account and media-permission declarations are owner items (R18); without them the pre-review leg stays not run | Owner | open | Medium |
