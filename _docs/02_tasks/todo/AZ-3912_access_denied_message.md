# [v1.1] macOS access-denied message in the settings window

**Task**: AZ-3912_access_denied_message
**Name**: Access-denied message
**Description**: When the daemon lists a pair in `DaemonStatus.access_denied`, the Status and Attention tabs show "access denied: allow access in System Settings" for that pair within 10 s, other pairs stay as they are, and the message clears when the daemon drops the pair from the list
**Complexity**: 1 point
**Dependencies**: AZ-3900_status_attention_screens, AZ-3919_macos_platform_fs_launchagent
**Component**: 15 desktop_settings
**Tracker**: AZ-3912
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `DaemonStatus.access_denied: [PairId]`, filled only by the macOS daemon (E-24/c05).

## Problem

AC-67 (v1.1): when macOS denies access to a folder, the tray and the settings window show "access denied: allow access in System Settings" within 10 s, and the other folders keep syncing. The daemon field now exists (Step 5 gap default, `tests/spec-gaps.md` section 1), so the window has something to render; IT-15-14 in the component tests still says BLOCKED.

## Outcome

- The message appears for each denied pair within 10 s of the daemon reporting it.
- Other pairs are shown unchanged; the message clears when the pair leaves the list.

## Scope

### Included
- The message on the Status tab and the Attention tab for each pair in `access_denied`.
- Clearing it when the pair is no longer listed.

### Excluded
- The tray tooltip (14, E-24/c21) and the permission probe itself.

## Acceptance Criteria

**AC-1: Message within 10 s (AC-67, IT-15-14)**
Given the window open and a stub that sends a status with `access_denied` = [P1]
When the event arrives
Then within 10 s the Status and Attention tabs show "access denied: allow access in System Settings" for P1, and the other pairs keep their normal state

**AC-2: Message clears**
Given the message shown for P1
When the stub sends a status without P1 in `access_denied`
Then the message for P1 disappears with that status

**AC-3: No message on Linux and Windows**
Given a status with an empty `access_denied`
When every tab is shown
Then no access-denied message appears

## Non-Functional Requirements

**Performance**
- Render within 10 s of the daemon reporting (AC-67); the refresh policy of E-25/c11 gives 1 s per event.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Status with and without denied pairs mapped to messages | One message per denied pair |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub on MAC, `access_denied` = [P1] | Event, time to message | <= 10 s; other pairs unchanged | IT-15-14, FT-P-75 (UI part) |
| AC-2 | Message shown | Status without P1 | Message gone | IT-15-14 |
| AC-3 | Empty list | All tabs | No message | |

## Constraints

- v1.1 only (macOS desktop, RESTRICT-02).

## Risks & Mitigation

**Risk 1: macOS lab not available**
- *Risk*: the H-MAC run of FT-P-75 is blocked.
- *Mitigation*: AC-1..AC-3 run with the stub; the real-daemon part stays with FT-P-75 (E-31).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `components/15_desktop_settings/tests.md` IT-15-14 and its AC-67 row still say BLOCKED although `access_denied` exists (spec-gaps section 1); doc fix needed | Doc owner | open | Low |
