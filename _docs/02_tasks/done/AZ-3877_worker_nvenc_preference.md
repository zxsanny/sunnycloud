# NVENC encoder preference in the policy step [v1.1]

**Task**: AZ-3877_worker_nvenc_preference
**Name**: Worker NVENC preference [v1.1]
**Description**: Plan `encoder: Nvenc` only when the operator set `nvenc = on` and the slot's decoder reported a successful NVENC probe; every other case plans `Cpu`, and an NVENC failure goes through the existing single CPU rerun
**Complexity**: 1 point
**Dependencies**: AZ-3876_worker_policy_step, AZ-3872_worker_slot_loop
**Component**: 09_worker_supervisor
**Tracker**: AZ-3877
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c10b

## Problem

AC-28 (v1.1) allows GPU encoding only when it is enabled and proven by a test encode; a missing or failing GPU must never fail a job. The supervisor is where the operator's switch and the decoder's probe meet.

## Outcome

- `Nvenc` plans appear only for (`nvenc = on`, `nvenc_probe_ok = true`).
- An `EncoderFailed` after an `Nvenc` plan reruns once on CPU (rule of E-20/c06).

## Scope

### Included
- `encoder_pref` from the `nvenc` config value and the cached `Capabilities.nvenc_probe_ok` of the slot, passed to `decide`.

### Excluded
- The decoder's probe and NVENC encode (E-19/c13); the rerun rule (E-20/c06).

## Acceptance Criteria

**AC-1: Preference matrix (IT-09-16, AC-28)**
Given `nvenc` off and on, and a decoder `hello` with `nvenc_probe_ok` true and false, and a non-compliant probe
When the transcode step is chosen in each of the four combinations
Then only (`on`, probe true) yields `encoder: Nvenc`; the other three yield `Cpu`

**AC-2: GPU failure reruns on CPU (IT-09-6 (c))**
Given an `Nvenc` plan answered with `EncoderFailed{exit_code: 1}`
When the slot handles it
Then it reruns once with `encoder: Cpu` and the job succeeds

## Non-Functional Requirements

**Compatibility**
- Default off; v1.0 behaviour is unchanged when `nvenc = off`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Preference from config and probe | `Nvenc` only for (on, true) |

## Blackbox Tests

Run in harness H2 with the fake decoder; the real GPU path is a system scenario.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | FD `hello` variants, config on/off | IT-09-16 | Matrix holds | FT-N-08 |
| AC-2 | FD `EncoderFailed` | IT-09-6 (c) | CPU rerun | NFT-RES-31, FT-P-28 |

## Constraints

- Release v1.1.

## Risks & Mitigation

**Risk 1: Stale probe after a decoder restart**
- *Risk*: the cached capability says true after the GPU disappeared.
- *Mitigation*: capabilities are refreshed by `hello` on every reconnect (E-20/c05); a failure still reruns on CPU.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
