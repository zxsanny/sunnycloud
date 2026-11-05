# Screen-reader reach of Change, Remove, checkbox and number field (NVDA, Narrator, Orca) (spike)

**Task**: AZ-3886_screen_reader_spike
**Name**: Screen-reader spike
**Description**: Check that NVDA, Narrator and Orca can reach and announce the four control kinds of the settings window built with Slint, record the accessibility properties needed, and record whether fallback trigger 3 fires
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 15 desktop_settings
**Tracker**: AZ-3886
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c03

## Problem

Screen-reader support of Slint widgets is unverified (15 §15 risk 2). ADR-012 names "NVDA, Narrator or VoiceOver cannot reach Change, Remove, a checkbox or a number field, and `accessible-*` properties cannot fix it" as fallback trigger 3. The UI automation tests (IT-15-2) also rely on the same accessibility tree to find controls by name and role.

## Outcome

- A reach matrix (control kind x screen reader) recorded in `components/15_desktop_settings/description.md` §15 risk 2.
- The accessibility properties each control kind needs, written for the screen tasks (E-25/c07..c11).
- A verdict on fallback trigger 3.

## Scope

### Included
- A prototype window with a Change button, a Remove button, a checkbox and a number field, built with the software renderer and accessibility enabled.
- NVDA and Narrator on Windows 11; Orca on Ubuntu GNOME.
- Keyboard reach (Tab order), announced name, role and value (checkbox state, number value).
- The same controls in the AT-SPI and UI Automation trees (name and role), as IT-15-2 needs.

### Excluded
- VoiceOver (macOS, v1.1).
- Accessibility of the full screens (each screen task and E-25/c14).

## Acceptance Criteria

**AC-1: Reach matrix recorded**
Given the four control kinds and the three screen readers
When each control is reached by keyboard
Then the record states per pair whether the control is reachable and whether its name, role and value (checkbox state, number) are announced

**AC-2: Fix or trigger**
Given any failure in the matrix
When `accessible-*` properties are applied
Then a fixed failure is recorded with the property that fixed it, and a failure that cannot be fixed is recorded as fallback trigger 3 for the owner

**AC-3: Automation tree**
Given the same prototype
When the AT-SPI tree (Linux) and the UI Automation tree (Windows) are dumped
Then each of the four controls appears with a name and a role that a test can search for

## Non-Functional Requirements

**Compatibility**
- Results are tied to the Slint version planned for release (1.18.1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: document outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Prototype; NVDA, Narrator (WIN-VM), Orca (LIN-VM) | Keyboard reach and announcements | Matrix recorded | IT-15-2 (manual part) |
| AC-2 | Failures from AC-1 | Apply properties | Fixed with property, or trigger 3 recorded | ADR-012 |
| AC-3 | Prototype | Tree dumps | Four controls with name and role | IT-15-2 |

## Constraints

- Manual screen-reader checks are recorded with the reader and OS versions.

## Risks & Mitigation

**Risk 1: A control cannot be made reachable**
- *Risk*: trigger 3 fires late, after the screens are built.
- *Mitigation*: this spike runs before E-25/c07..c11 start.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| "Change" is not tied to a named control in 15 §5 or the mockup (no "Change" button is drawn); the spike uses a generic push button for it | E-25 / doc owner | open | Low |
