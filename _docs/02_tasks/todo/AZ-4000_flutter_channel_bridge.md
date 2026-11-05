# Flutter channel bridge: status events, commands and the no-token check

**Task**: AZ-4000_flutter_channel_bridge
**Name**: Flutter channel bridge
**Description**: The Kotlin and Dart halves of the Method/EventChannel: a full status snapshot on every change, typed commands to native code, and an automated check that no token, code or verifier can cross to Dart
**Complexity**: 3 points
**Dependencies**: AZ-3991_backup_core_surface, AZ-4002_native_library_ci
**Component**: 17 android_app (Kotlin and Dart sides of the channel)
**Tracker**: AZ-4000
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `BackupStatus`, `ServerCheck`, `BackupError` (E-27/c07).
- `_docs/02_document/contracts/17_android_app/status_channel.md` — produced by this task.

## Problem

Kotlin owns the `BackupCore` handle; Flutter only displays state and sends commands (S4-4). Android and iOS share the Dart screens, so the channel must be one written schema that Swift (18) can implement too. Tokens must never reach Dart (17 §11, RESTRICT-33). Without a fixed schema, each screen would invent fields and the iOS port would drift.

## Outcome

- Kotlin side: subscribes to core status, sends a full snapshot on every change and on subscribe, and handles every command of `status_channel.md` by calling `BackupCore` or the platform.
- Dart side: a typed client for the event stream and the commands, used by the screens (E-27/c15, c17).
- A schema check in tests: the channel types contain no token-shaped field, and runtime capture finds no sentinel value.

## Scope

### Included
- The event stream and command set of `status_channel.md`; typed errors mapped from `BackupError`.
- Native settings storage for `unmetered_only`, `charging_only` and buckets, read by the WorkManager constraints (E-27/c10) and the asset source (E-27/c09).
- The schema and sentinel checks.

### Excluded
- Screens (E-27/c15, c17); the Swift implementation of the same channel (E-28).

## Acceptance Criteria

**AC-1: Snapshot on subscribe and on change**
Given a core with a completed run
When Dart subscribes, and then one more asset is uploaded
Then Dart receives one full snapshot at once and another after the upload, each with the counts of `BackupStatus`

**AC-2: Commands reach native code with typed results**
Given each command of `status_channel.md`
When Dart sends it with valid and with invalid arguments
Then valid calls return the stated result, `setServer("http://...")` returns `HttpsRequired`, and invalid settings return `Invalid{field}` without changing stored values

**AC-3: No token can cross (ST-17-1 channel leg)**
Given the channel schema and a sign-in plus run with sentinel tokens
When the schema check runs and all events and command results are captured
Then the schema has no field named like a token, code or verifier, and the capture holds 0 sentinel matches; a seeded token field makes the check fail

**AC-4: Settings stored natively**
Given Wi-Fi only off and an album deselected through `updateSettings`
When native readers (the entry points of E-27/c10, the asset source of E-27/c09) read the settings, and again after an app restart
Then they get the new values both times, and the next status snapshot shows them

## Non-Functional Requirements

**Performance**
- A snapshot is small (counts and short strings only) and sent at most once per status change.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Command handlers with valid and invalid input | Stated results and typed errors |
| AC-3 | Schema check on the real schema and a seeded token field | Pass, then fail |

## Blackbox Tests

Flutter integration test on the emulator with the real Kotlin side.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Completed run | Subscribe, upload one more | Two snapshots | IT-17-19 |
| AC-3 | Sentinel tokens | Capture all traffic | 0 matches | ST-17-1, RESTRICT-33 |
| AC-4 | Settings changed | Native read, restart | New values kept | IT-17-19 |

## Constraints

- Dart never calls Rust; Kotlin is the only holder of the UniFFI handle (ADR-013).

## Risks & Mitigation

**Risk 1: iOS drifts from the schema**
- *Risk*: Swift sends different field names.
- *Mitigation*: E-28 consumes `status_channel.md`, not this task.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Mockup values without a `BackupStatus` field (last backup time, time left, account name, storage used/total) are not in the schema (`status_channel.md` open point 1) | Owner / E-27 | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/17_android_app/status_channel.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
