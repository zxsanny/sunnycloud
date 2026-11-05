# Native dialog behaviour per OS: GNOME without zenity, wlroots without portal, Windows (spike)

**Task**: AZ-3884_rfd_dialog_os_spike
**Name**: Native dialog spike
**Description**: Record how the native folder, file and confirm dialogs behave on Windows, GNOME with and without a portal backend or zenity, and a wlroots desktop without a portal, when driven from the Slint event loop, so the dialog wrapper and its no-backend message can be built on facts
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 15 desktop_settings
**Tracker**: AZ-3884
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c02

## Problem

AC-17 requires every folder or file choice to open the operating system's native dialog. The planned library needs an XDG portal backend or zenity on Linux and does not use wlroots portals (15 §5). Runtime behaviour per OS is unverified (15 §7): what a call returns when no backend exists, whether the dialog is parented and modal to the settings window, and whether focus returns. ADR-012 lists "dialog deadlock or unusable focus under the winit loop" as fallback trigger 4.

## Outcome

- A per-OS behaviour matrix recorded in `components/15_desktop_settings/description.md` §7.
- The way the window tells "user cancelled" from "no dialog backend" written down for E-25/c12.
- A verdict on fallback trigger 4 (fired or not).

## Scope

### Included
- Folder pick, file pick and confirm, awaited on the Slint event loop, on: Windows 11; Ubuntu GNOME with the portal; GNOME with neither portal backend nor zenity; a wlroots desktop (for example Sway) without a portal backend.
- Per case: dialog opens or not, the result of the call, parenting and modality, focus after close, and how the Linux portal call can be observed on the session bus (the IT-15-3 method).
- 20 open and cancel cycles per case to look for hangs.

### Excluded
- The production wrapper and the in-window message (E-25/c12).
- macOS (v1.1).

## Acceptance Criteria

**AC-1: Behaviour matrix recorded**
Given the four OS setups
When folder pick, file pick and confirm are called from the event loop
Then 15 §7 records, per setup and call, whether the dialog opens, what the call returns, whether the dialog is parented and modal, and where focus goes after close

**AC-2: Cancel versus no backend**
Given a cancelled dialog and a setup with no backend
When both results are compared
Then the record states how the window can tell the two apart, or states that it cannot, so E-25/c12 knows when to show the no-backend message

**AC-3: No hang under repeated use (fallback trigger 4)**
Given each setup
When 20 open and cancel cycles run
Then any hang or lost focus is recorded with steps to reproduce, and fallback trigger 4 is recorded as fired for the owner if it cannot be avoided

**AC-4: Observation method for IT-15-3**
Given GNOME with the portal and Windows 11
When a folder dialog opens
Then the record names what IT-15-3 observes: the `org.freedesktop.portal.FileChooser` call on the session bus, and the common dialog window on Windows

## Non-Functional Requirements

**Reliability**
- A hang is any dialog call that does not return within 5 s of the dialog closing.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: document outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WIN-VM, LIN-VM GNOME (portal / none), Sway without portal | Three dialog calls per setup | Matrix recorded | IT-15-3, FT-P-50 |
| AC-2 | Cancel; no backend | Compare results | Distinction recorded | IT-15-3 |
| AC-3 | Each setup | 20 cycles | Hangs recorded; trigger 4 verdict | ADR-012 |
| AC-4 | GNOME, Windows | Observe dialog | Observation method recorded | IT-15-3 |

## Constraints

- No custom path browser may be proposed as a fallback (AC-17).

## Risks & Mitigation

**Risk 1: No backend on minimal desktops**
- *Risk*: users on wlroots or minimal GNOME cannot pick a folder.
- *Mitigation*: the record feeds the in-window message and the package notes of E-25/c12.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The 5 s hang threshold is chosen by this spike; no spec sets one | E-25 | open | Low |
