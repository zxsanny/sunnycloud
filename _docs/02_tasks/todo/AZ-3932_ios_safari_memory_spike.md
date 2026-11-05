# Spike: iOS Safari memory ceilings for hashing very large files

**Task**: AZ-3932_ios_safari_memory_spike
**Name**: iOS Safari hashing memory spike
**Description**: Measure memory, throughput and tab survival while a module Worker hashes very large files block by block in iOS Safari and Mac Safari, and record the ceiling and its consequences for the upload engine
**Complexity**: 2 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3652_initial_structure
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3932
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c03

## Problem

iOS Safari ends tabs at lower memory than desktop browsers and suspends background tabs (16 §7). The upload engine reads 2 MiB with `Blob.slice` and hashes each block in WASM, assuming memory stays flat for any file size (16 §15 risk 4, epic risk 4). Nobody has measured whether that holds on an iPhone, or how large a file can be hashed before the tab is killed.

## Outcome

- Measured peak memory, hash throughput and tab survival on iPhone Safari and Mac Safari for files of 1 GiB, 5 GiB and the largest size the test device can store.
- The ceiling and its consequences recorded in `components/16_web_app/description.md` §7 (caveats) and §15 risk 4.
- E-26/c10 knows whether block-sized WASM calls are enough or whether an iOS size message is needed (owner decides whether that is a product limit).

## Scope

### Included
- The E-26/c01 probe extended to hash a selected file in a module Worker, one 2 MiB block per WASM call.
- Runs on an iPhone 12 or newer and on the owner Mac in Safari; memory from Web Inspector and `vmmap` footprint (weak evidence, labelled).

### Excluded
- Upload over the network (E-26/c10); Android browsers (not in AC-50).

## Acceptance Criteria

**AC-1: Measured per size and device**
Given the probe hashing in a module Worker with one 2 MiB block per WASM call
When files of 1 GiB, 5 GiB and the largest size the device can store are hashed on iPhone Safari and Mac Safari
Then the record holds peak memory, throughput and whether the tab survived, per size and device

**AC-2: Flat memory checked**
Given the same runs
When memory is sampled every 5 s
Then the record states whether memory stays flat over the run (no growth with bytes hashed), which is the condition E-26/c10 relies on

**AC-3: Ceiling and consequence recorded**
Given the measurements
When the spike closes
Then the 16 description §7 and §15 risk 4 state the largest size hashed without a tab kill and whether the 150 MB heap target holds; if a limit below 50 GiB exists on iOS, the record proposes the user message and lists it as an open owner item for E-26/c10

## Non-Functional Requirements

**Performance**
- Upload heap target 120 MB, failure above 150 MB (AC-50) is the reference the record compares against.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Probe loop releases each block before reading the next | Memory sample series shows no growth on a 1 GiB desktop run |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | iPhone 12 or newer, owner Mac, Safari | Hash 1 GiB, 5 GiB, largest stored file | Values recorded; flatness stated | AC-50, PT-16-1 |

## Constraints

- Hashing stays in the Worker; nothing runs on the main thread.

## Risks & Mitigation

**Risk 1: No test iPhone available**
- *Risk*: the hardware inventory (E-31/c01) names no phone yet.
- *Mitigation*: record `NOT-RUN(HW-DEVICE)` for iOS; the Mac Safari values are still recorded.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No iPhone is named in the hardware inventory yet (`tests/environment.md`, E-31/c01) | Owner (hardware) | open | Medium |
| Whether an iOS size limit, if found, is a product limit or a documented caveat | Owner | open | Low |
