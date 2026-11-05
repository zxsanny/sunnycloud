# NativeDialogs: OS folder, file and confirm dialogs, and the no-backend message

**Task**: AZ-3909_native_dialogs
**Name**: Native dialogs
**Description**: One wrapper for `pick_folder`, `pick_file` and `confirm(text)` that always opens the operating system's dialog parented to the settings window, shows a chosen path read-only, and on Linux without a portal backend or zenity shows a message naming that requirement instead of any fallback browser
**Complexity**: 2 points
**Dependencies**: AZ-3884_rfd_dialog_os_spike, AZ-3895_settings_app_shell
**Component**: 15 desktop_settings
**Tracker**: AZ-3909
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c12

## Problem

AC-17 forbids any custom file browser: every folder or file choice must open the OS dialog (FT-P-50). On Linux the dialog needs an XDG portal backend or zenity; without one the window must explain the requirement and keep running, never draw its own browser (15 §5, §7). The destructive hold decision also needs a native confirmation (E-25/c11).

## Outcome

- Every path choice in the window goes through this wrapper and opens the OS dialog.
- With no backend the window shows a message naming the XDG portal or zenity and does not crash.
- The Linux package notes state the requirement (epic Definition of Done).

## Scope

### Included
- `pick_folder() -> Option<path>`, `pick_file() -> Option<path>`, `confirm(text) -> bool`, awaited on the UI event loop and parented to the settings window, with the behaviour recorded by E-25/c02.
- A cancelled dialog returns nothing and shows nothing.
- The no-backend message, using the distinction between cancel and no backend recorded by E-25/c02.
- The package note for minimal Linux desktops (portal backend or zenity required).

### Excluded
- The screens that call the wrapper (E-25/c08, E-25/c11).
- macOS (v1.1).

## Acceptance Criteria

**AC-1: OS dialog every time (AC-17, IT-15-3)**
Given the window on Windows 11 and on Ubuntu GNOME with the portal
When `pick_folder` and `pick_file` are called
Then on Windows the common dialog window is found, on Linux the `org.freedesktop.portal.FileChooser` call is seen on the session bus, and the chosen path appears in a read-only field

**AC-2: No custom browser (AC-17, FT-P-50)**
Given every screen of the window
When the whole accessibility tree is walked
Then 0 list or tree widgets show local directory contents; the server-folder picker of the Folders tab, which lists server folders, is the only folder list

**AC-3: No backend (IT-15-3)**
Given Linux with neither a portal backend nor zenity
When the user chooses a folder
Then the window shows a message that names the XDG portal or zenity requirement, keeps running and offers no fallback browser

**AC-4: Confirm**
Given `confirm("Delete 200 items on the server?")`
When the user accepts, and in a second run declines
Then the call returns true and false

**AC-5: Package note**
Given the Linux package documentation
When it is read
Then it states that a portal backend or zenity is required for folder selection

## Non-Functional Requirements

**Reliability**
- A dialog call never blocks the UI thread (ADR-012 fallback trigger 4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Result classification: picked, cancelled, no backend | Path, nothing, message |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | WIN-VM, LIN-VM with portal | Folder and file picks | OS dialog observed; read-only path | IT-15-3, FT-P-50 |
| AC-2 | All screens | Tree walk | 0 local directory listings | IT-15-3, FT-P-50 |
| AC-3 | LIN-VM without portal and zenity | Choose folder | Message; no crash | IT-15-3 |
| AC-4 | Window | Confirm accept and decline | true, false | IT-15-6 |
| AC-5 | Package docs | Review | Requirement stated | DoD |

## Constraints

- No custom path browser anywhere (AC-17, 15 §2).

## Risks & Mitigation

**Risk 1: Portal missing on wlroots desktops**
- *Risk*: folder selection fails for those users.
- *Mitigation*: AC-3 message and AC-5 package note.

## Runtime Completeness

- Named capability: OS-native file dialogs through the planned dialog library.
- Production code that must exist: real calls to the OS dialog on Linux and Windows.
- Allowed external stubs: none in AC-1..AC-4.
- Unacceptable substitutes: an in-window list of local folders, a text field as the only way to choose a local folder.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No screen of 15 §5 uses `pick_file`; FT-P-50 names an "exclude-rules import" that no spec defines. The wrapper provides `pick_file` per 15 §2 but no entry point is invented | E-25 / doc owner | open | Low |
| IT-15-3 says "0 widgets list directory contents"; the server-folder picker lists server folders and must be exempt in the test wording | Doc owner | open | Low |
