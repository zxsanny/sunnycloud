# Decoder integration suite IT-10-1..20 on the production container (H3) with the red-team corpus

**Task**: AZ-3865_decoder_integration_suite
**Name**: Decoder integration suite (H3)
**Description**: Run the decoder test cases against both decoder containers started with production flags in the test stack, build the `test-stubs` variant for hostile behaviour, and add the container-only cases: self-check variants, seccomp, restart after hostile outcomes, the watchdog abort and the red-team corpus
**Complexity**: 5 points
**Dependencies**: AZ-3852_decoder_protocol_module, AZ-3853_decoder_runner_socket_loop, AZ-3854_decoder_process_guard, AZ-3855_decoder_containment_kill_all, AZ-3856_decoder_safe_open_watchdog, AZ-3857_decoder_image_executor, AZ-3858_decoder_probe, AZ-3859_decoder_video_poster, AZ-3860_decoder_video_executor, AZ-3861_decoder_hello_self_check, AZ-3863_decoder_media_build_stages, AZ-3864_decoder_image_build_checks, AZ-3850_decoder_s4_heic_stubs_spike, AZ-3851_decoder_watchdog_hook_ssim_spike, AZ-3742_test_harness_skeleton; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); E-31/c09 (test epic E-31, decomposed at greenfield Step 9); E-31/c10 (test epic E-31, decomposed at greenfield Step 9); E-31/c20 (test epic E-31, decomposed at greenfield Step 9); E-31/c21 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 10_decoder
**Tracker**: AZ-3865
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — the harness is a protocol client built from the shared module.
- `_docs/02_document/components/10_decoder/tests.md` — harness H3, IT-10-1..20 expected results.

## Problem

Containment only means something in the container as shipped: `network_mode: none`, read-only root, `cap_drop ALL`, `no-new-privileges`, the seccomp profile, cgroup limits and `restart: unless-stopped`. Several cases cannot run anywhere else (self-check variants, `EPERM` from seccomp, the restart after a hostile outcome), and hostile behaviour on demand needs stub binaries (G-10-1). The suite is the decoder's evidence for AC-25 and AC-71 at its own boundary, with no supervisor code involved.

## Outcome

- Harness H3: both decoder containers from the test stack with production flags, the socket volume mounted into the harness, test durations from config.
- The `test-stubs` build target per the S4 decision, absent from the release image.
- IT-10-1..20 (except the v1.1 NVENC part of IT-10-20) pass against the production containers.

## Scope

### Included
- H3 setup: `decoder-interactive` (tmpfs scratch) and `decoder-general` (disk scratch), production flags and limits, harness client from `media-shared::decoder_protocol`, per-test job directories created with mode 0440 and group `decoder_gid`; test config `decoder_stall_kill=3s`, `decoder_alive_interval=500ms`, `decoder_watchdog=3s`, `decoder_memory_poll_interval=100ms`.
- The `test-stubs` target: stub `vipsthumbnail`/`ffmpeg` selecting hang, allocate, ignore signals, `setsid` plus double fork, write to the next job's path, `ptrace`, 1 MiB stderr from a marker in `in/source`; a check that the release image digest has no stub.
- Container recreation between groups that end hostile-class; teardown checks (scratch empty, no stray process).
- All of IT-10-1 to IT-10-19 with the expected results of `tests.md`; IT-10-18 with the trigger of E-19/c07b; the self-check variants of IT-10-2; IT-10-20 for the DS-16 corpus and the controls.
- Reference outputs (SSIM, packet hashes, ffprobe) from the harness's own tools, never from the runner's report.

### Excluded
- PT-10-x, ST-10-x, AT-10-x (E-19/c16); the NVENC part of IT-10-20 (E-19/c13, v1.1); system scenarios (E-31).

## Acceptance Criteria

**AC-1: Production flags before any case (NFT-SEC-29 step 1-2)**
Given the test stack
When the suite starts
Then both decoder containers are inspected first and have `network_mode: none`, read-only root, `cap_drop ALL`, `no-new-privileges`, the seccomp profile, `pids_limit`, and cgroup limits of 2 CPU / 2 GiB (interactive) and 4 CPU / 4 GiB (general); a mismatch stops the suite

**AC-2: Stubs exist only in the test variant (G-10-1)**
Given the release decoder image and the `test-stubs` variant
When both are inspected
Then the stub binaries and marker handling exist only in the variant

**AC-3: Protocol, limits and containment cases pass (AC-25, AC-71)**
Given H3
When IT-10-1, IT-10-3, IT-10-12, IT-10-13, IT-10-14, IT-10-15, IT-10-16, IT-10-17, IT-10-18 and IT-10-19 run
Then each meets its expected result in `tests.md`, including restart by Compose after every hostile-class outcome and `hello` answering again within 30 s

**AC-4: Media cases pass on production flags (AC-10, AC-11, AC-44, AC-45)**
Given H3 with DS-03, DS-05 base, DS-06 base, DS-08 and DS-35
When IT-10-4 to IT-10-11 run
Then each meets its expected result, with the HLG parts of IT-10-7 and IT-10-10 recording their values (G-10-6, S5)

**AC-5: Self-check variants (IT-10-2)**
Given the production container and the five variants
When `self_check` runs in each
Then production passes every item and each variant flips exactly its own item

**AC-6: Red-team corpus (AC-25, IT-10-20)**
Given every DS-16 item and the control image and control video
When each is sent as a job
Then every hostile item ends in a typed `DecodeError`, no output or log line contains `root:x:0:0` or environment contents, the 20 GB sparse video ends within its limits, and both controls succeed afterwards

**AC-7: Clean teardown**
Given the end of each test group
When the harness checks the containers
Then the scratch volumes are empty and no process other than the runner exists

## Non-Functional Requirements

**Reliability**
- Each case keeps the "Max execution time" of `tests.md`; IT-10-20 is bounded at 20 min.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Stub marker parsing in the stub binaries | Each marker selects its behaviour |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Test stack | Container inspection | Production flags | NFT-SEC-29, NFT-RES-LIM-04 |
| AC-3 | H3, stubs, ctl | IT-10-1, 3, 12, 13, 14, 15, 16, 17, 18, 19 | As `tests.md` | NFT-SEC-45, NFT-RES-36, AC-71 |
| AC-4 | H3, DS-03/05/06/08/35 | IT-10-4..11 | As `tests.md` | FT-P-22..27 |
| AC-5 | Container variants | IT-10-2 | One flip per variant | NFT-SEC-29 |
| AC-6 | DS-16, controls | IT-10-20 corpus part | Typed failures, no exposure | NFT-SEC-28, NFT-RES-LIM-04, FT-N-07 |
| AC-7 | End of each test group | Teardown inspection | Scratch empty; only the runner process | AT-10-1 step 3 |

## Runtime Completeness

- Named capability: the decoder image as shipped (real libvips, libheif, ffmpeg) under the production container flags.
- Allowed external stubs: the `test-stubs` binaries, only in cases marked `stub` in `tests.md`.
- Unacceptable substitutes: running media cases on the stub variant, on a host without the container flags, or with a fake runner.

## Constraints

- The harness plays the supervisor; no 09 code is involved.
- Container-level tiers run on the host named in `tests/environment.md`; a local run does not count as the tier result.

## Risks & Mitigation

**Risk 1: Long corpus items slow CI**
- *Risk*: the 20 GB sparse video and 4K cases take tens of minutes.
- *Mitigation*: run them in the nightly tier and keep fast cases in the PR tier (tier wiring E-31/c17).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Compose services of the test stack (E-31/c05) must carry the decoder seccomp profile of E-19/c07a, otherwise AC-1 and IT-10-15 fail | E-31 owner | open | Medium |
