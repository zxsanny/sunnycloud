# hello capabilities, self_check and image build facts

**Task**: AZ-3861_decoder_hello_self_check
**Name**: Decoder hello and self-check
**Description**: Fill `Capabilities` (protocol, library versions, image build timestamp, NVENC fields) for `hello`, and answer `self_check` with one stable, data-only item per isolation property of the running container
**Complexity**: 2 points
**Dependencies**: AZ-3853_decoder_runner_socket_loop, AZ-3863_decoder_media_build_stages
**Component**: 10_decoder
**Tracker**: AZ-3861
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `hello`, `self_check`, `Capabilities`, `SelfCheckReport`.

## Problem

The supervisor takes no job until each decoder proves its isolation (AC-25 self-check gate), and the operator's `doctor` shows the decoder image age so the weekly rebuild can be checked (AC-63, via 09 status). The decoder reports facts; the supervisor decides. A self-check that errors or merges items would hide which property failed.

## Outcome

- `hello{protocol: 1}` returns the real library versions and build timestamp of the running image.
- `self_check` returns one named item per property and never fails as a call.

## Scope

### Included
- `Capabilities{protocol, vips, ffmpeg, libheif, build_timestamp, nvenc_probe_ok, nvenc_sessions}`: versions read from the installed tools, build timestamp from the image build facts written by E-19/c14a; `nvenc_probe_ok = false` and `nvenc_sessions = 0` while `nvenc = off` (the probe is E-19/c13).
- `self_check(forbidden_targets)`: a connect and a resolve attempt per target (all must fail), environment equals the allowlist, mount table lacks the blob path and secrets, uid is not 0, effective capabilities are 0, `no_new_privs` set, root filesystem read-only.
- Stable item names; failed items are data, not errors.

### Excluded
- The gate decision and status report (E-20/c05, E-20/c08); container variants for the tests (E-19/c15).

## Acceptance Criteria

**AC-1: Capabilities (IT-10-1)**
Given the decoder image
When `hello{protocol: 1}` is sent
Then `protocol` is 1, the vips, ffmpeg and libheif versions are non-empty and equal the tools' own version output, `build_timestamp` is the image's build time and not in the future, `nvenc_probe_ok` is false and `nvenc_sessions` is 0

**AC-2: Production container passes every item (IT-10-2)**
Given the production container and `forbidden_targets = [api:8080, api:8081, db:5432, 1.1.1.1:53, example.com:443]`
When `self_check` runs
Then every item is `ok`

**AC-3: Each variant flips only its own item (IT-10-2)**
Given variants with `cap_add: NET_RAW`, uid 0, a writable root filesystem, an extra variable `X_TEST=1`, and the blob path bind-mounted
When `self_check` runs in each
Then exactly the matching item is `ok = false`, the others stay `ok`, the call never errors, and item names are the same strings in every run

## Non-Functional Requirements

**Performance**
- `self_check` completes within the IT-10-2 budget (30 s for six container starts).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Each check function against crafted mount tables, status files and environments | One item flips per input |
| AC-1 | Version parsing from recorded tool output | Non-empty versions |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Decoder image | IT-10-1 capability part | Real versions and timestamp | AC-63 |
| AC-2, AC-3 | Production container and five variants (run in E-19/c15) | IT-10-2 | All ok; one flip per variant | AC-25, NFT-SEC-29 |

## Constraints

- The decoder does not decide: a failed item is reported, the supervisor refuses jobs (09 §3).

## Risks & Mitigation

**Risk 1: A check passes because it cannot see the property**
- *Risk*: a missing file reads as "no blob mount".
- *Mitigation*: AC-3 variants prove each item can fail.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The blob path and secret paths the mount check looks for are taken from the deployment docs (`/var/lib/sunnycloud/blobs`, `/run/secrets`); no config key names them | E-19 owner | open | Low |
