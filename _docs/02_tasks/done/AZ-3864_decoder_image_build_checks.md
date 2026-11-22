# Decoder image build checks in CI (loaders, protocols, encoders, versions, SBOM, source offer)

**Task**: AZ-3864_decoder_image_build_checks
**Name**: Decoder image build checks
**Description**: A repository check, run by the CI `images` job on every decoder build, that fails the build when the image gains an excluded loader, protocol, demuxer or encoder, loses a required one, drifts from the pinned versions, or lacks its SBOM or source offer
**Complexity**: 3 points
**Dependencies**: AZ-3863_decoder_media_build_stages, AZ-3761_ci_skeleton
**Component**: 10_decoder
**Tracker**: AZ-3864
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c14b

## Dependencies

### Document Dependencies
- `_docs/02_document/deployment/containerization.md` — "Build checks that run in CI on every build".

## Problem

A version bump or a recipe change can silently re-enable `hls` or an SVG loader, or pull in a non-free encoder, which reopens the AC-25 attack surface or breaks the GPL distribution (RESTRICT-29). The properties must be checked on every build, not only at release.

## Outcome

- Every decoder build in CI runs the check; any deviation fails the job with the failing property named.
- The same check serves the weekly rebuild and the release workflow.

## Scope

### Included
- ffmpeg: configuration has `--enable-gpl` and not `--enable-nonfree`; encoders list `aac` and `libx264`, lack `libfdk_aac` and `libx265`; protocols only `file` and `pipe`; demuxers lack `hls` and `concat` (and the other excluded demuxers of E-19/c14a).
- libheif at least 1.22.0; libvips exactly 8.18.7; libvips loaders lack svg, pdf and magick.
- SBOM present and listing every source-built component with name, version and CPE.
- Source-offer notice and OCI label present and pointing to the source asset of this build.
- Wiring into the CI `images` job for the decoder image.

### Excluded
- The scan for CVEs and the scanner canary (supply-chain and release jobs); the image build itself (E-19/c14a).

## Acceptance Criteria

**AC-1: A good image passes**
Given the decoder image built by E-19/c14a
When the check runs in the `images` job
Then every property passes and the job succeeds

**AC-2: Each property can fail (seeded)**
Given images built with, in turn, the `hls` demuxer enabled, the SVG loader enabled, `libfdk_aac` present, libvips at another version, and the SBOM removed
When the check runs on each
Then each run fails and names the failing property

**AC-3: Licence properties (FT-P-74)**
Given the release decoder image and the repository root
When the FT-P-74 steps run
Then `--enable-libx264` and `--enable-gpl` are present, neither `--enable-libfdk-aac` nor `--enable-nonfree` is present, `aac` is listed, and the source offer for GPL components is in the image

## Non-Functional Requirements

**Reliability**
- The check needs no network; it reads only the image.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | The check's parsing of recorded tool outputs (good and seeded bad) | Pass and named failure per case |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | CI `images` job | Check on the built image | Pass | AC-63 |
| AC-2 | Seeded images | Check | Named failure each | AC-25 |
| AC-3 | Release image, repository | FT-P-74 | All 3 checks pass | RESTRICT-29 |

## Constraints

- CI calls repository scripts only (E-00/c09 pattern); actions pinned by SHA.

## Risks & Mitigation

**Risk 1: Tool output format changes with a version bump**
- *Risk*: the check stops matching and passes vacuously.
- *Mitigation*: AC-2 seeded failures run in CI, so a check that cannot fail is caught.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Split from E-19/c14 (3 points planned) because the media build stages it checks had no owner; the check itself keeps the planned 3 points | Parent (Step 4 ref map) | open | Low |
