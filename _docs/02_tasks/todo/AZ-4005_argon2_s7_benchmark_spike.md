# Spike S7: Argon2id timing and memory on a low-end phone and Safari; set the WASM and mobile caps [v1.2]

**Task**: AZ-4005_argon2_s7_benchmark_spike
**Name**: Argon2 S7 benchmark
**Description**: Measure Argon2id unlock time and peak memory with the pinned crate natively on a low-end Android phone and in WASM on iOS and Mac Safari, then record the default parameters and the WASM and mobile memory caps that keep unlock within 1 s native and 2 s in the browser
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4005
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c02

## Dependencies

### Document Dependencies
- `components/19_vault_core/description.md` §3 (KDF bounds, S7), §11 risk 2; `components/19_vault_core/tests.md` IT-19-5 (BLOCKED part), PT-19-1; `tests/spec-gaps.md` row IT-19-5; `acceptance_criteria.md` AC-14, AC-61.

## Problem

Unlock must take at most 1 s native and 2 s in the browser (AC-14) with Argon2id at 64 MiB, t = 3, p = 1. Nobody has measured that on a low-end phone or in Safari. The header bounds also need a memory cap per platform: 512 MiB native is stated, but the WASM and mobile caps are left to this benchmark (IT-19-5 BLOCKED). A cap set too high lets a hostile header exhaust a phone or a tab; too low and a vault created on a desktop cannot be opened on a phone.

## Outcome

- Measurements recorded in `components/19_vault_core/description.md` §3 and in the IT-19-5 row of `tests/spec-gaps.md` (resolved with the date): unlock time (p50, p95) and peak memory per device for m = 64, 128, 256 MiB at t = 3 and 4, p = 1.
- Decided: default `KdfParams`, the WASM memory cap and the mobile memory cap, each with its reason.
- Unblocks E-29/c06 (bounds constants) and E-29/c14 (PT-19-1 parameters).

## Scope

### Included
- A throwaway probe: the pinned `argon2 =0.6.0` (no `parallel`) as a native binary on the phone (through `adb`) and as WASM in a module Worker in Safari.
- Devices: one low-end Android phone (owner hardware), iPhone 12 or newer Safari, Mac Safari, plus the REF-SRV class CPU as the native baseline.

### Excluded
- The real KDF code (E-29/c07); the browser 250 MB total of AC-14 (E-31, NFT-RES-LIM-08).

## Acceptance Criteria

**AC-1: Measurements per device**
Given the probe on each listed device
When 20 derivations are run per parameter set
Then the record shows p50 and p95 time and peak memory per device and parameter set

**AC-2: Defaults and caps decided**
Given the measurements
When the spike ends
Then the record states the default parameters, the WASM cap and the mobile cap, each within 64 MiB to 512 MiB, each keeping p95 unlock at most 1 s native and 2 s in the browser on the measured devices, and the IT-19-5 row of `tests/spec-gaps.md` is resolved

**AC-3: Missing hardware is visible**
Given a device that is not available
When the spike ends
Then its legs are `NOT-RUN(HW-DEVICE)` or `NOT-RUN(H-IOS)`, and no cap is decided from a workstation run alone

## Non-Functional Requirements

**Performance**
- Single-threaded WASM (no threads, no COOP/COEP), as in production.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (measurement record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Phone, iPhone Safari, Mac Safari, REF-SRV | 20 derivations per set | Times and memory recorded | AC-14, NFT-PERF-29 |
| AC-2 | Measurements | Record review | Defaults and caps with reasons | AC-61 |

## Constraints

- The 64 MiB floor and p = 1 are fixed by AC-61; the spike only lowers caps or keeps defaults within the budget.

## Risks & Mitigation

**Risk 1: Low-end phone misses 1 s at 64 MiB**
- *Risk*: the floor itself is too slow.
- *Mitigation*: the record states it; the owner decides (AC-14 and AC-61 conflict), parameters never go below the floor.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The "low-end phone" model is not named in 19 or the test environment | Owner | open | Low |
| The phone, iPhone and Mac are owner hardware; legs may stay not run | Owner | open | Low |
