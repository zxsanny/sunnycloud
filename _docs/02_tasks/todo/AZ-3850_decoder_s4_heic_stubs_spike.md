# Gate S4: HEIC 1440 px timing, test-stubs mechanism and 30 s base files

**Task**: AZ-3850_decoder_s4_heic_stubs_spike
**Name**: Decoder gate S4 spike
**Description**: Measure HEIC preview and thumbnail time under the interactive limits, decide how the 320 px HEIC thumb is made, and settle how tests substitute `vipsthumbnail`/`ffmpeg` (G-10-1) and get the 30 s base clips (G-10-3)
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure; notes: E-31/c20 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 10_decoder
**Tracker**: AZ-3850
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c02

## Problem

A 1440 px HEIC preview needs a full HEVC decode, which may miss the 5 s thumbnail budget at p95 (AC-10, 10 risk 1). The hostile-behaviour tests need `vipsthumbnail` and `ffmpeg` to hang, allocate, fork or attack on demand, but no build target or config key for such substitutes exists (G-10-1). Several video tests need the 30 s base clips, while the generators only expose 10-minute loops (G-10-3).

## Outcome

- Measured HEIC pair times on the interactive limits and a recorded decision on the 320 px source (embedded thumbnail, full decode, or derived from the preview).
- Verified `vipsthumbnail` option strings for orientation, metadata stripping and output quality.
- A recorded test-stubs mechanism that E-19/c15 builds, and a recorded manifest requirement for the 30 s base files that the video generator (E-31/c21) provides.

## Scope

### Included
- Time 20 HEIC and 100 JPEG of DS-03 at 1440 px then 320 px with 2 CPU and 2 GiB; record p50 and p95 per pair.
- Verify the option strings of 10 §4 step 3 (auto-rotate, `keep=none`, no `--linear`, compiled libheif limits).
- Decide the stub mechanism (build target name, how stub binaries replace the real ones, marker convention in `in/source`, proof that the release image contains no stub) and record it in `tests/spec-gaps.md` (G-10-1 closed by decision) and the component description.
- Record the manifest entries (file names, durations, hashes) needed for DS-05 and DS-06 base files against E-31/c21 (G-10-3).

### Excluded
- Building the stub target (E-19/c15) and the generator change itself (E-31/c21).
- The image executor (E-19/c08).

## Acceptance Criteria

**AC-1: HEIC timing measured (AC-10)**
Given 20 HEIC and 100 JPEG of DS-03 and a decoder limited to 2 CPU and 2 GiB
When each is rendered at 1440 px and then 320 px
Then p50 and p95 per pair are recorded against PT-10-1 (JPEG 600 ms target, 2 s failure; HEIC 1.5 s target, 3 s failure)

**AC-2: Thumb source decided**
Given the AC-1 numbers
When the spike closes
Then the component description states how the 320 px HEIC thumb is produced and why, with the measured cost of each option

**AC-3: Option strings verified**
Given orientation 6 and 8 JPEGs and a HEIC with GPS tags
When rendered with the recorded option strings
Then dimensions swap for 5-8, no EXIF/GPS/ICC strings remain in the output, and the strings are recorded for E-19/c08

**AC-4: Stub mechanism recorded (G-10-1)**
Given the hostile behaviours the tests need (hang, allocate, ignore SIGTERM, `setsid` plus double fork, write to the next job's path, `ptrace`, 1 MiB stderr)
When the spike closes
Then `tests/spec-gaps.md` marks G-10-1 resolved with the mechanism and the release-image check, naming E-19/c15 as the builder

**AC-5: Base files requested (G-10-3)**
Given IT-10-7, IT-10-8, IT-10-10, IT-10-13 and AT-10-2
When the spike closes
Then the 30 s base file entries for DS-05 and DS-06 are written as a requirement on E-31/c21 and G-10-3 points to it

## Non-Functional Requirements

**Performance**
- HEIC pair p95 target 1.5 s, failure 3 s (PT-10-1); end to end 5 s p95 (NFT-PERF-13).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2, AC-4, AC-5 | Spike record review (component description, `tests/spec-gaps.md`, E-31/c21 requirement) | Each decision recorded where the AC names it |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-03, 2 CPU / 2 GiB | Pair timing | Numbers recorded | PT-10-1, NFT-PERF-13 |
| AC-3 | Orientation and GPS samples | Option strings | Swap and stripping hold | AC-10 |

## Constraints

- Timing on a workstation is informational; gate S4 numbers count on the hardware named in `tests/environment.md`.

## Risks & Mitigation

**Risk 1: HEIC misses the budget**
- *Risk*: full HEVC decode is slower than 3 s at p95.
- *Mitigation*: deriving the 320 px from the preview halves the decodes; recorded as the fallback in AC-2.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The data generators belong to E-31; this spike only records the requirement, so G-10-3 stays open until E-31/c21 ships the base files | E-31 owner | open | Low |
