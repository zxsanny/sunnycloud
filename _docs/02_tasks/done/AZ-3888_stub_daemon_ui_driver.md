# Stub daemon with scripted answers and the UI automation driver

**Task**: AZ-3888_stub_daemon_ui_driver
**Name**: Stub daemon and UI driver
**Description**: A test-only JSON-RPC server that answers every `DaemonControl` method from scripted answer files, records each request, emits status events and can send hostile frames, plus a small UI automation driver (AT-SPI on Linux, UI Automation on Windows) that every settings-window test uses
**Complexity**: 5 points
**Dependencies**: AZ-3889_ipc_schema_crate, AZ-3652_initial_structure
**Component**: 15 desktop_settings (test tooling)
**Tracker**: AZ-3888
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — methods, types, errors, wire mapping (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — the behaviour the stub's default answers imitate (E-24/c19).

## Problem

The settings window has no server dependency; its only boundary is the daemon's JSON-RPC endpoint. A stub at that process boundary is the right test double (15 tests.md, "Isolation"). Every blackbox test of E-25 needs it, with scripted errors, delays, pages and events, and with exact request recording. The same tests drive the real window through the accessibility tree, so they also need one driver that works on Linux and Windows. The epic row plans 3 points for the stub alone; the estimate is raised to 5 because the driver is needed by every IT test before the suite task (E-25/c14) exists, and it must work on two platforms.

## Outcome

- A stub daemon that answers every method of the daemon control interface, by default the way `daemon_control.md` describes, and per test from a scripted answer file.
- Exact request records and per-method counts that tests can assert on.
- A UI driver that launches the window against a given endpoint, finds controls by accessible name and role, clicks, types, reads values, dumps the tree and measures time to the first interactive frame, with the same test scripts on Linux and Windows.

## Scope

### Included
- A local endpoint owned by the current user with the modes of `ipc_schema.md` (Transport), on a temporary path per test.
- Scripted answers per call: a result, any `ControlError` variant, `Internal{correlation_id}`, a delay, or no answer at all.
- `subscribe` notifications on a scripted schedule, and a command to stop them.
- Paged answers for `list_server_folders`, `list_skipped` and `list_escaped`.
- Hostile frames on demand: a 10 MB answer, invalid JSON, a wrong id, error text with markup (`<b>x</b>{@html}<script>`).
- Request records (method and params exactly as received) and counts per method.
- The UI driver for AT-SPI (Linux, under Xvfb or a Wayland session) and UI Automation (Windows).

### Excluded
- The squatter endpoints owned by another OS user (E-25/c05, IT-15-12).
- Real-daemon runs (E-25/c14).
- Any client logic of the window.

## Acceptance Criteria

**AC-1: Every method answered**
Given the stub with its default script
When each method listed in `ipc_schema.md` is called
Then each returns a decodable result of its declared type, and a coverage test fails when the schema gains a method the stub does not answer

**AC-2: Defaults match the daemon's behaviour**
Given the test cases of `daemon_control.md` (for example `set_server("http://x")`, `set_settings` with bandwidth -1, `resolve_hold` on an active pair)
When the stub answers them with its default script
Then each answer equals the contract's expected result (`HttpsRequired`, `SettingInvalid{key}` with settings unchanged, `NoSuchHold`)

**AC-3: Scripted errors, delays and silence**
Given a script with `RulesInvalid{line: 3}`, `Internal{correlation_id: "c-77"}`, a 4 s delay and a call that is never answered
When a client makes those calls
Then it receives exactly those errors, the delayed answer arrives after the scripted delay, and the silent call gets no frame

**AC-4: Exact recording (IT-15-4)**
Given a rules text with a trailing newline and non-ASCII characters
When the client sends it in `update_pair`
Then the stub's record holds the string byte-equal to what was sent, and the per-method count is 1

**AC-5: Events on a schedule (IT-15-9)**
Given a script that emits a `StatusEvent` every second for 10 s and then stops
When a client subscribes
Then it receives the 10 events in order, each carrying a full `DaemonStatus`, and none after the stop

**AC-6: Hostile frames (ST-15-1)**
Given a script for each hostile frame
When the client calls the scripted method
Then the stub sends the frame exactly as scripted (10 MB answer, invalid JSON, wrong id, markup text)

**AC-7: One driver on two platforms**
Given the settings window binary and a stub endpoint on H-LIN (AT-SPI) and on H-WIN (UI Automation)
When the same test script launches the window, finds a control by accessible name and role, clicks it, types into a field, reads the field's value and dumps the tree
Then every step succeeds on both platforms and the dump lists each control with its name and role

## Non-Functional Requirements

**Performance**
- The stub adds no delay of its own to an unscripted answer, so the 5 s timeout tests measure the window, not the stub.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Method coverage against the schema's method list | No method missing |
| AC-3 | Script parser with errors, delays, silence | Parsed into the scripted actions |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Default script | Call every method | Decodable results | ipc_schema round-trip case |
| AC-2 | `daemon_control.md` cases | Default answers | Equal to the contract | IT-14-15 |
| AC-3 | Error, delay, silence script | Client calls | Exact errors; delay; no frame | IT-15-1, IT-15-5 |
| AC-4 | Rules text fixture | `update_pair` | Byte-equal record | IT-15-4 |
| AC-5 | Event script | `subscribe` | 10 events, none after stop | IT-15-9 |
| AC-6 | Hostile scripts | Calls | Frames sent as scripted | ST-15-1 |
| AC-7 | H-LIN, H-WIN | Driver script | All steps pass on both | IT-15-2 |

## Constraints

- The stub is test tooling and is never shipped or linked into the settings binary.
- The stub uses the `ipc_schema` crate for its types, never a copy (ipc_schema invariant).

## Risks & Mitigation

**Risk 1: The stub drifts from the real daemon**
- *Risk*: UI tests pass against answers the daemon never gives.
- *Mitigation*: AC-2 ties the defaults to `daemon_control.md`; E-25/c14 reruns the key flows against the real daemon.

## Runtime Completeness

- Named capability: UI automation through AT-SPI and UI Automation.
- Production code that must exist: a driver that acts through the OS accessibility tree of the real window binary.
- Allowed external stubs: the stub daemon itself (it sits at the process boundary 15 owns).
- Unacceptable substitutes: calling the window's view model directly instead of the accessibility tree, or a headless renderer that bypasses the real window.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Estimate raised from 3 to 5 points to include the UI driver for both platforms (needed before E-25/c14) | E-25 | open | Low |
