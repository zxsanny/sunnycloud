# Settings screen: one call per change, the daemon's echo shown, revert on failure or 5 s silence

**Task**: AZ-3899_settings_screen
**Name**: Settings screen
**Description**: The Settings tab: bandwidth limit, initial-hash throttle, start at login, show tray icon, pause all and language; each change is one `set_settings` call, the control shows the value the daemon echoes with an "Applied" mark, and a refused or unanswered change reverts within 5 s
**Complexity**: 3 points
**Dependencies**: AZ-3888_stub_daemon_ui_driver, AZ-3890_daemon_client, AZ-3895_settings_app_shell
**Component**: 15 desktop_settings
**Tracker**: AZ-3899
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `get_settings`, `set_settings`, `Settings`, `SettingsPatch`, `SettingInvalid{key}` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — echo semantics; a patch with an invalid key changes nothing (E-24/c19).

## Problem

AC-16 requires a changed setting to take effect in the daemon within 5 s without a restart, and 15 §5 requires no hidden state: the window shows only what the daemon confirms. If the daemon refuses a value, clamps it, or does not answer, the control must not keep showing a value that was never applied (FT-P-82, from the approved mockup's Settings tab).

## Outcome

- Every change is exactly one `set_settings` call holding only the changed key.
- The control shows the echoed value and an "Applied" mark for 2 s (DESIGN.md §6).
- `SettingInvalid{key}` or no answer within 5 s returns the control to the last applied value with an inline message.

## Scope

### Included
- Controls of 15 §5 and the mockup: bandwidth limit (number with Apply, "0 = unlimited"), initial-hash throttle ("Go easy on the first scan", on/off), start at login, show tray icon (helper "Takes effect the next time sunnycloud starts"), pause all, language (English only, disabled with the reason "Only English is available in this version").
- Load with `get_settings` at open; one `set_settings` per change; the echoed `Settings` drives every control.
- The "Applied" mark for 2 s after an echo; on failure the control reverts and an inline message names the setting; on a timeout the message says the daemon did not answer within 5 seconds.
- Accessible names and roles for every control (checkboxes and the number field included).

### Excluded
- What each setting does in the daemon (E-24/c19, c17).
- The real-daemon run of FT-P-82 and PT-15-3 (E-25/c14).

## Acceptance Criteria

**AC-1: One call, echo shown (AC-16, IT-15-5)**
Given a stub that echoes `Settings` with `bandwidth_limit` 4096 for a request of 5000
When the user sets the bandwidth limit to 5000 and applies
Then exactly one `set_settings` call is sent holding only the bandwidth key, the field shows 4096, and the "Applied" mark shows for 2 s

**AC-2: Refused change reverts (IT-15-5)**
Given a stub answering `SettingInvalid{key: bandwidth_limit}`
When the user applies a new bandwidth value
Then the field returns to the last applied value and an inline message names the bandwidth setting

**AC-3: Unconfirmed change reverts within 5 s (AC-16, IT-15-5, FT-P-82 step 1)**
Given a stub that does not answer `set_settings`
When the user toggles "Start at login"
Then within 5 s the checkbox returns to its previous value and an inline message says the daemon did not answer within 5 seconds

**AC-4: Each control sends its own key**
Given the stub
When the user toggles start at login, show tray icon and the hash throttle one after another
Then three `set_settings` calls are sent, each holding only its own key, and each control shows its echoed value

**AC-5: No hidden state (IT-15-5)**
Given applied changes
When the window is closed and opened again
Then every control shows the values of `get_settings`

**AC-6: Language is display-only**
Given the language control
When the user tries to change it
Then it is disabled with its reason and no call is sent

## Non-Functional Requirements

**Performance**
- The echoed value is shown within 1 s of the daemon's answer (PT-15-3: UI shows applied <= 1 s, fail > 5 s).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Patch built from one changed control | Holds only that key |
| AC-2 | Revert on refusal and timeout | Control returns to the last applied value |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub echo 4096 | Apply 5000 | One call; 4096 shown; Applied | IT-15-5 |
| AC-2 | Stub `SettingInvalid` | Apply | Revert; message names key | IT-15-5 |
| AC-3 | Stub silent | Toggle start at login | Revert within 5 s; did-not-answer message | IT-15-5, FT-P-82 |
| AC-4 | Stub | Three toggles | Three single-key calls | IT-15-5 |
| AC-5 | Applied values | Reopen | Values from `get_settings` | IT-15-5 |
| AC-6 | Language control | Try to change | Disabled; no call | DESIGN.md §6 |

## Constraints

- No state of its own; the daemon's echo is the only source of a shown value (15 §5).

## Risks & Mitigation

**Risk 1: Revert hides a late success**
- *Risk*: the daemon applies a change after the window's 5 s timeout, so the window shows the old value.
- *Mitigation*: the next status event or `get_settings` shows the daemon's real value; the window never shows a value the daemon did not report.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-AS-2: the bandwidth limit has no unit (15 §5; tests use "5000"; the mockup shows KB/s; `ipc_schema.md` open point 6) | Owner | open | Medium |
| "Pause all" is listed in 15 §5 and drawn in the mockup, but `SettingsPatch` has no such key and `pause`/`resume` take one pair; the call that carries it must be fixed in `ipc_schema` before this control is built | E-24 / E-25 owners | open | Medium |
| The hash throttle is on/off in 15 §5 but a number (`f64`) in `ipc_schema.md`; the mapping of on and off to values is not stated | E-24 / E-25 owners | open | Low |
