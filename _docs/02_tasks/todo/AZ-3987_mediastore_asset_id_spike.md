# Spike: MediaStore asset ID stability on in-place overwrite

**Task**: AZ-3987_mediastore_asset_id_spike
**Name**: MediaStore asset ID spike
**Description**: Measure on the emulator and three phones whether editing a photo in place keeps its MediaStore asset ID and how its change token moves, so the AC-46 linking rule rests on evidence
**Complexity**: 2 points
**Dependencies**: None
**Component**: 17 android_app (MediaStore behaviour)
**Tracker**: AZ-3987
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c03

## Dependencies

### Document Dependencies
- `acceptance_criteria.md` AC-46 (an edit is linked when the same MediaStore asset ID has a new content hash); epic assumption "Overwriting a photo in place keeps its MediaStore asset ID"; epic risk 5.

## Problem

AC-46 links an edited photo to its original only when the MediaStore asset ID stays the same and the content hash changes. If an OEM gallery saves an edit as a new row, the edit becomes an unlinked new file. The epic assumes the ID is stable and says an OEM that changes it is reported, not worked around. Nobody has measured it.

## Outcome

- A record in `components/17_android_app/description.md` §7 (caveats): per device and per edit method, asset ID before and after, generation before and after, whether the content hash changed.
- Devices that change the ID are named with the consequence for AC-46.
- Input for E-27/c06 (change-token handling) and E-27/c14 (linking).

## Scope

### Included
- A throwaway probe that reads asset ID, generation and content hash before and after an edit.
- Edit methods: the stock gallery editor's save on each device, and an in-place write through the content resolver.
- Emulator images API 30, 33 and 34; Pixel, Samsung, Xiaomi (H-ANDROID).

### Excluded
- Any workaround for an OEM that changes the ID (the epic forbids it).
- The linking logic itself (E-27/c14).

## Acceptance Criteria

**AC-1: Measurements per device and method**
Given each emulator image and phone, and each edit method
When a photo is edited and saved in place
Then the record shows asset ID, generation and content hash before and after the edit

**AC-2: Unstable IDs reported, not worked around**
Given a device whose edit produces a new asset ID
When the spike ends
Then the record names the device and states that its edits upload as unlinked new files under AC-46, and no code change is proposed for it

**AC-3: Missing hardware is visible**
Given a phone that is not available
When the spike ends
Then that device's legs are `NOT-RUN(HW-DEVICE)`, and the emulator legs are still recorded

## Non-Functional Requirements

**Compatibility**
- Emulator images API 30, 33, 34; minSdk 30.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (measurement record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Emulator images and phones | In-place edit per method | IDs, generations, hashes recorded | AC-46 |
| AC-2 | Device with a new ID | Record review | Device named; no workaround | AC-46 |
| AC-3 | Missing phone | Record review | `NOT-RUN(HW-DEVICE)` | |

## Constraints

- Phones are owner hardware (R03).

## Risks & Mitigation

**Risk 1: Behaviour differs by app version**
- *Risk*: a gallery update changes the result.
- *Mitigation*: the record names the gallery app version per device.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Phones are owner hardware; the device legs may stay `NOT-RUN(HW-DEVICE)` | Owner | open | Low |
