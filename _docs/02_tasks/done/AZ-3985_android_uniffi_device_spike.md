# Spike: one UniFFI library per ABI on device (NDK r28, 16 KB pages, JNA, forced kills)

**Task**: AZ-3985_android_uniffi_device_spike
**Name**: Android single-library device spike
**Description**: Prove on the emulator and one phone that one Rust library per ABI loads through UniFFI and JNA on 16 KB-page images, survives forced kills, and record the build recipe and the app's process model
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 17 android_app (`mobile_backup` library build)
**Tracker**: AZ-3985
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — open point 1 (Android platform-verifier initialisation) is decided by E-23/c15 together with this spike.
- ADR-013 (UniFFI only, one library per ABI); `components/17_android_app/description.md` §5 (Native library), §15 risk 1.

## Problem

Every Android upload runs in one Rust library per ABI that holds `mobile_backup` and sync-core, called from Kotlin through UniFFI (ADR-013). Android 15 devices can use 16 KB memory pages, and a library linked for 4 KB pages fails to load there; JNA needs at least 5.17.0 for this. Nobody has built such a library with the pinned NDK, loaded it from a WorkManager worker, or watched what a `kill -9` in the middle of a library call does to an open SQLite database (R31). The camera-backup pipeline also assumes the worker and the UI share one process, so the in-process run mutex covers both (AZ-3673 risk 1); that is not yet checked.

## Outcome

- A written record in `components/17_android_app/description.md` §5 and §15 risk 1: NDK version, ABI list, link setting for 16 KB pages, JNA version, UniFFI version, library size per ABI, load time.
- Measured behaviour after forced kills during library calls.
- The process model stated with evidence: worker and UI in one process, or not.
- The platform-verifier initialisation mechanism agreed with E-23/c15 and handed to it.
- Unblocks E-27/c05 and E-27/c18 (crate and library build), E-27/c12 (trust bridge), E-28/c03 (iOS static library); the probe code is not merged.

## Scope

### Included
- A throwaway probe: an empty library with one exported UniFFI call and a write loop to a WAL SQLite file, plus a minimal app that loads it from the UI and from a WorkManager worker.
- Builds with the pinned NDK (r28 or newer) for the phone and emulator ABIs.
- Load checks on an emulator image with 16 KB pages and one with 4 KB pages, minSdk 30.
- A forced-kill loop (rooted emulator) during library calls.
- One physical Pixel leg (H-ANDROID).

### Excluded
- The real crate, the UniFFI surface and the CI job (E-27/c05, c07, c18).
- The sync-core UniFFI derives and the verifier hook itself (E-23/c15).

## Acceptance Criteria

**AC-1: The library loads on 16 KB and 4 KB pages**
Given the probe library built per ABI with the pinned NDK and the 16 KB link setting, and JNA 5.17.0 or newer
When the probe app loads it and calls the export from the UI and from a worker on a 16 KB-page emulator image and on a 4 KB-page image
Then every call returns the expected value, and the alignment check reports every loadable segment of every library aligned to 16 KB

**AC-2: Forced kills leave a usable database**
Given the probe app running a loop of library calls that write to a WAL SQLite file
When the process is killed with `kill -9` 20 times at random moments and restarted each time
Then every restart loads the library, opens the file without error and passes the integrity check, and no committed write is missing

**AC-3: Process model recorded**
Given the probe app with a running worker and the UI open
When the process ids of both are read
Then the record states whether they share one process and names the manifest setting that keeps it so, which answers AZ-3673 risk 1

**AC-4: Decisions recorded**
Given the measurements above
When the spike ends
Then the record lists NDK version, ABIs, link setting, JNA and UniFFI versions, library size and load time per ABI, the verifier-initialisation mechanism agreed with E-23/c15, and the follow-ups for E-27/c05, c18 and E-28/c03

**AC-5: Phone leg**
Given a physical Pixel
When AC-1 and five kills of AC-2 are repeated on it
Then the results are recorded; without the phone the leg is `NOT-RUN(HW-DEVICE)`, never PASS

## Non-Functional Requirements

**Compatibility**
- minSdk 30; UniFFI 0.32.2 from the workspace lockfile.

**Reliability**
- Build and check commands print their progress; no quiet flags.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Alignment check on the built libraries and on a library seeded with 4 KB alignment | Pass, then fail |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 16 KB and 4 KB emulator images | Load and call from UI and worker | Calls return; segments 16 KB aligned | R31 |
| AC-2 | Rooted emulator, write loop | 20 forced kills | Library loads; file opens; integrity ok | AC-52 |
| AC-3 | Worker running, UI open | Process ids | One process (or the finding recorded) | AC-78 |
| AC-5 | Pixel | AC-1 and AC-2 legs | Recorded or `NOT-RUN(HW-DEVICE)` | |

## Constraints

- UniFFI only; no flutter_rust_bridge (ADR-013, S4-4).
- Probe code stays out of the main branch.

## Risks & Mitigation

**Risk 1: The worker runs in its own process**
- *Risk*: a manifest or library default puts WorkManager in another process, so the in-process mutex does not serialize runs.
- *Mitigation*: AC-3 records it; AC-78 still holds through idempotent uploads (E-27/c06).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The ABI list is not stated in 17 (phones and the x86_64 emulator are assumed); the spike records the list it proves | E-27 owner | open | Low |
| Physical phones are owner hardware (R03); the device leg may stay `NOT-RUN(HW-DEVICE)` | Owner | open | Low |
