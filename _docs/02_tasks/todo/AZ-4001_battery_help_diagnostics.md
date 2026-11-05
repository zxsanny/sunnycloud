# Battery and OEM help pages, ring-buffer logs and diagnostics export with confirmation

**Task**: AZ-4001_battery_help_diagnostics
**Name**: Battery help and diagnostics
**Description**: Show the battery-exemption state with per-vendor help (Xiaomi, Samsung, Huawei), keep the app's logs in a bounded ring buffer with no file names or tokens, and export them only after the user confirms
**Complexity**: 2 points
**Dependencies**: AZ-3988_oem_background_s9_spike, AZ-3999_flutter_backup_ui, AZ-4000_flutter_channel_bridge
**Component**: 17 android_app (Flutter help pages, Kotlin and Rust logging)
**Tracker**: AZ-4001
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/status_channel.md` — `battery_exempt`, `requestBatteryExemption`, `exportDiagnostics` (E-27/c16).
- Records of E-27/c02 (battery-exemption request policy) and E-27/c04 (vendor help steps).
- `components/17_android_app/description.md` §5 (Battery/OEM), §9 (Logging).

## Problem

OEM task killers delay or drop background work (R32); users need vendor-specific steps (frame 6 of the mockup: "Battery optimised · May delay", "Steps for Samsung, Xiaomi, Huawei"). Logs help support but must never hold file names or tokens, and must leave the phone only after the user agrees (17 §9, RESTRICT-33). No row owns the bounded log store that the export reads, so this 1-point row is re-estimated at 2.

## Outcome

- Battery section on the settings screen with the exemption state and the action allowed by Play policy (E-27/c02).
- Help pages per vendor with the steps recorded by E-27/c04.
- A bounded ring-buffer log file in app-private storage for the Rust and Kotlin layers (Logcat in debug builds only): INFO run start and end with counts and pause reasons, WARN skipped assets and permission changes, ERROR token rejected and database errors; asset ids only at DEBUG; no file names, no tokens.
- "Share diagnostics" exports the ring buffer only after the user confirms.

## Scope

### Included
- Battery state display and action; vendor help pages.
- Ring-buffer logging with a fixed size cap and the level rules above.
- The export flow with a confirmation dialog.
- Test ST-17-4.

### Excluded
- The battery and help content's measurements (E-27/c04).

## Acceptance Criteria

**AC-1: Battery state and help**
Given the app with battery optimisation on, then exempted
When the settings screen opens
Then it shows "Battery optimised" with the allowed action in the first case and the exempt state in the second, and the Samsung, Xiaomi and Huawei help pages show the recorded steps

**AC-2: Logs hold no names or tokens (ST-17-4)**
Given sentinel file names and sentinel tokens, and the runs of the wire-error and folder cases (IT-17-15, IT-17-11)
When the ring buffer and Logcat are inspected
Then the sentinels occur 0 times, and asset ids appear only at DEBUG level

**AC-3: Export only after confirmation (ST-17-4)**
Given "Share diagnostics"
When the user declines once and accepts once
Then nothing leaves the app after the decline, and the ring buffer is handed to the share sheet only after the accept

**AC-4: Bounded size**
Given a long run that writes far more log lines than the cap
When the ring buffer is read
Then its size never exceeds the cap and the newest lines are kept

## Non-Functional Requirements

**Reliability**
- Logging failures never stop a backup run and are not silent: they are counted and shown in diagnostics.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Log formatting with sentinel names and tokens | 0 sentinels; ids only at DEBUG |
| AC-4 | Ring buffer over the cap | Size bounded; newest kept |

## Blackbox Tests

Emulator with the real app (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Optimisation on, then exempt | Settings screen | States and help pages shown | R32 |
| AC-2 | Sentinels; IT-17-15 and IT-17-11 runs | Inspect logs | 0 sentinels | ST-17-4, RESTRICT-33 |
| AC-3 | Decline, then accept | Share diagnostics | Export only after accept | ST-17-4 |

## Constraints

- No verbose debug logging in release builds; Logcat output in debug builds only (17 §9).

## Risks & Mitigation

**Risk 1: A third-party library logs a name**
- *Risk*: a dependency writes paths to Logcat.
- *Mitigation*: AC-2 inspects Logcat as well as the ring buffer.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The ring-buffer size cap is not stated in 17 §9 | E-27 owner | open | Low |
| Huawei steps stay unverified without a Huawei device (E-27/c04) | Owner | open | Low |
