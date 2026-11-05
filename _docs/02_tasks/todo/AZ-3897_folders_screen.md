# Folders screen: pair list, add with the server-folder picker or a typed path, pause and remove

**Task**: AZ-3897_folders_screen
**Name**: Folders screen
**Description**: The Folders tab: the list of sync pairs with their state, pause and resume, remove, and the add flow that takes the local folder from the native dialog and the server folder either from a paged picker or as a typed path created if missing, never both
**Complexity**: 5 points
**Dependencies**: AZ-3888_stub_daemon_ui_driver, AZ-3890_daemon_client, AZ-3895_settings_app_shell, AZ-3909_native_dialogs
**Component**: 15 desktop_settings
**Tracker**: AZ-3897
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `list_pairs`, `PairView`, `PairState`, `add_pair`, `PairSpec`, `PairTarget`, `list_server_folders`, `FolderPage`, `remove_pair`, `pause`, `resume`, `DaemonStatus.watcher_degraded` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — effects and errors of these methods (E-24/c19).

## Problem

Pairing a folder is the main task in this window. The local side must come from the OS dialog (AC-17). The server side comes from a picker fed by `list_server_folders`, or from a typed path that the daemon creates if missing; a `PairSpec` with both is invalid. The seven `add_pair` errors must appear beside the control that caused them. The pair list must show each pair's state as the approved mockup draws it.

## Outcome

- The pair list shows every pair with its state; pause, resume and remove are one call each.
- Adding a pair sends exactly one `add_pair` with exactly one server target.
- Errors appear beside the local or server control; `UnknownFolder` clears the server selection.

## Scope

### Included
- Pair rows: local root, the server side, and a state chip per `PairState` (`Active`, `Paused`, `HoldMassDelete` as "Deletes on hold", `RootMissing` as "Folder missing"), plus "Rescan only" for a pair listed in `watcher_degraded` (mockup Folders tab).
- Pause or Resume per pair; Remove per pair (`remove_pair`).
- Add form: "Choose folder…" opens the native folder dialog (E-25/c12) and shows the path read-only; the server picker pages through `list_server_folders`, loads the next page on scroll and opens nested folders; or "Type a path" with the note "Created on the server if it doesn't exist".
- Submit sends `add_pair` with `PairTarget::NodeId` for a picked folder or `PairTarget::Path{create_if_missing: true}` for a typed path, and an empty rules text (rules are set in the Rules tab, AT-15-1 step 3); the form refuses to submit when both are set.
- Error placement: `RootInvalid`, `RootNested`, `CloudManagedRoot` beside the local folder; `AlreadyPaired`, `UnknownFolder` beside the server folder; `NotSignedIn` as a form message pointing to the Account tab.
- Empty state with one primary action "Add folder pair" (DESIGN.md §6); accessible names and roles for every control.

### Excluded
- The rules editor (E-25/c09); the hold decision (E-25/c11).
- The dialog wrapper itself (E-25/c12).

## Acceptance Criteria

**AC-1: Pair list with states**
Given a stub status with four pairs (Active and listed in `watcher_degraded`, Paused, `HoldMassDelete`, `RootMissing`)
When the Folders tab opens
Then each pair shows its local root, its server side and the chip of its state, and the first also shows "Rescan only"

**AC-2: Pause, resume, remove**
Given an active pair and a paused pair
When the user pauses the first, resumes the second and removes one pair
Then exactly one `pause`, one `resume` and one `remove_pair` call is sent with the right pair id, and the list follows the next status event

**AC-3: Local folder only through the OS dialog (AC-17)**
Given the add form
When the user presses "Choose folder…" and picks a folder
Then the native dialog opens, the chosen path appears in a read-only field, and the form holds no widget that lists local directory contents

**AC-4: Server picker with paging (IT-15-7)**
Given a stub `list_server_folders` that returns 3 pages and a nested folder
When the user scrolls the picker and opens and picks the nested folder
Then further pages load on scroll and `add_pair` carries `PairTarget::NodeId` of the picked folder only

**AC-5: Typed path, never both (IT-15-7)**
Given the add form
When the user types `Photos/2026`, and in another attempt picks a folder and also types a path
Then the first sends `PairTarget::Path{server_path: "Photos/2026", create_if_missing: true}`, and the second cannot be submitted and sends no call

**AC-6: Errors beside the causing control (IT-15-1)**
Given a stub answering `add_pair` in turn with `RootInvalid`, `RootNested`, `CloudManagedRoot`, `AlreadyPaired`, `UnknownFolder` and `NotSignedIn`
When the user submits six times
Then the first three appear beside the local folder, the next two beside the server folder, `NotSignedIn` as a form message pointing to the Account tab, and `UnknownFolder` also clears the server selection

## Non-Functional Requirements

**Performance**
- The list reflects a status event within 1 s (15 tests IT-15-9 rule, applied to this tab).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Form to `PairSpec`: picked only, typed only, both, neither | One target, or submit refused |
| AC-6 | Error-to-control mapping for the seven `add_pair` errors | Each maps to its control |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub status with four pairs | Open tab | Chips per state; "Rescan only" | IT-15-2 |
| AC-2 | Stub pairs | Pause, resume, remove | One call each; list follows | FT-P-49 step 1 |
| AC-3 | Add form | Choose folder | Native dialog; read-only path; no local listing | IT-15-3, FT-P-50 |
| AC-4 | Stub with 3 pages | Scroll, open nested, pick | Pages load; node id only | IT-15-7 |
| AC-5 | Add form | Typed path; both set | Path target; submit refused | IT-15-7 |
| AC-6 | Stub error script | Submit six times | Errors beside controls; selection cleared | IT-15-1 |

## Constraints

- No custom file browser for local folders (AC-17); the server picker lists server folders only.
- One `DaemonControl` call per user action (15 §5).

## Risks & Mitigation

**Risk 1: Remove taken for delete**
- *Risk*: a user fears Remove deletes files.
- *Mitigation*: `remove_pair` deletes neither local nor server files (`daemon_control.md` invariant); see the flagged concern on a confirmation.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `PairView` carries `server_node_id` only, while the mockup shows the server path ("/Documents"); no method returns the path of a pair's server folder | E-24 / E-25 owners | open | Medium |
| Whether Remove asks for a confirmation is not specified (DESIGN.md §5 draws it as a danger-ghost button); `remove_pair` keeping files is itself `daemon_control.md` open point 1 | E-24 / owner | open | Low |
