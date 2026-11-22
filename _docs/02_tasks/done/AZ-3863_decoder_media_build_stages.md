# Decoder image media build: pinned sources, trimmed libvips/libheif/ffmpeg, SBOM and source offer

**Task**: AZ-3863_decoder_media_build_stages
**Name**: Decoder media build stages
**Description**: Turn the decoder image skeleton into the real untrusted image: fetch pinned, hash-verified sources, build libheif, libvips and ffmpeg with the licence and attack-surface trimming of the deployment docs, and ship the component SBOM and the GPL source offer
**Complexity**: 5 points
**Dependencies**: AZ-3741_docker_compose_skeletons, AZ-3761_ci_skeleton, AZ-3849_decoder_s5_whitelist_hdr_spike, AZ-3850_decoder_s4_heic_stubs_spike, AZ-3652_initial_structure
**Component**: 10_decoder
**Tracker**: AZ-3863
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c14a

## Dependencies

### Document Dependencies
- `_docs/02_document/deployment/containerization.md` — Decoder section (stages, versions, ffmpeg and libvips build flags, source offer, SBOM).
- `_docs/02_document/deployment/ci_cd_pipeline.md` — media layer cache key and the `images` job.

## Problem

The decoder is only as safe as what is compiled into it: an ffmpeg with `hls` or `concat` demuxers can be pointed at `/etc/passwd`, a libvips with SVG or PDF loaders parses documents, and a non-free encoder breaks the GPL distribution (AC-25, AC-63, RESTRICT-29). E-00/c11 built the image skeleton with the runner only and assigned the media build stages, the version pin list, the ffmpeg configure recipe, the SBOM and the source offer to E-19; no other E-19 row owned them.

## Outcome

- The decoder image contains libheif >= 1.22.0, libvips 8.18.7 and the pinned ffmpeg, built from hash-verified sources with the documented flags.
- The image lacks every loader, protocol and demuxer the docs exclude.
- The image carries a source-built SBOM, the GPL source offer, and the build facts the runner reports in `hello`.

## Scope

### Included
- A version pin list with source URL and SHA-256 per tarball; the fetch stage fails on any mismatch.
- Build stage for libheif (decode only: no x265, AOM, rav1e or SVT encoders; libde265 and dav1d), libvips (loader allowlist jpeg, png, webp, heif, gif plus what S4 shows phones need; SVG, PDF, ImageMagick, OpenSlide, FITS, NIfTI, Matlab, OpenEXR compiled out) and ffmpeg (`--enable-gpl --enable-libx264 --enable-libzimg`, native `aac`; no `--enable-nonfree`, `libfdk_aac`, `libx265`, CUDA/NPP; `--disable-autodetect --disable-network --disable-devices --disable-doc --disable-ffplay`; protocols `file` and `pipe` only; demuxer allowlist from gate S5; no `hls`, `dash`, `concat`, `tee`, `data`).
- Runtime stage on the pinned Debian base with only runtime libraries (libx264, libde265, dav1d, libwebp, libjpeg-turbo, libpng, libexif, libzimg, lcms2), user 10003, entrypoint `decoder-runner`.
- A component SBOM listing every source-built component with name, version and CPE.
- The source-offer notice and the OCI label pointing to the matching source asset of that build.
- Build facts for the runner: library versions and the image build timestamp.

### Excluded
- The check script (E-19/c14b); pushing, signing, scanning and the weekly rebuild workflow (release workflows); the runner code (E-19/c05 to c12); the `test-stubs` target (E-19/c15).

## Acceptance Criteria

**AC-1: Hash-verified sources**
Given the version pin list
When the image is built, and again with one SHA-256 changed by one character
Then the first build succeeds and the second fails in the fetch stage

**AC-2: Libraries at the pinned versions**
Given the built image
When the installed versions are read
Then libheif is at least 1.22.0, libvips is 8.18.7 and ffmpeg is the pinned tag

**AC-3: Trimmed ffmpeg (AC-25, RESTRICT-29)**
Given the built image
When ffmpeg reports its configuration, encoders, protocols and demuxers
Then the configuration has `--enable-gpl` and `--enable-libx264` and lacks `--enable-nonfree`; encoders include `aac` and `libx264` and lack `libfdk_aac` and `libx265`; protocols are only `file` and `pipe`; demuxers lack `hls`, `dash`, `concat`, `tee` and `data`

**AC-4: Trimmed libvips**
Given the built image
When libvips lists its loaders
Then no SVG, PDF or ImageMagick loader is present

**AC-5: SBOM and source offer (AC-63)**
Given the built image
When its documentation folder, SBOM and OCI labels are read
Then the SBOM lists libheif, libvips and ffmpeg with name, version and CPE, and the source-offer notice and label name the source asset of this build

**AC-6: Runtime image contents**
Given the built image
When its contents are listed
Then it runs as user 10003 with entrypoint `decoder-runner`, contains no compiler or source tree, and is under 700 MB

## Non-Functional Requirements

**Performance**
- The media layer is cached on the pin list and recipe hash (first build about 25 min, cached afterwards).

**Security**
- 0 known HIGH or CRITICAL CVEs in ffmpeg, libheif, libvips and libwebp at release (AC-63, enforced by the release scan, not this task).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (build recipe; verified by building) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Pin list, seeded bad hash | CI `images` job | Build passes, then fails at fetch | AC-63 |
| AC-2, AC-3, AC-4, AC-5 | Built image | Check script of E-19/c14b | All checks pass | FT-P-74, AC-25 |
| AC-6 | Built image | Inspect user, entrypoint, size | As specified | |

## Runtime Completeness

- Named capability: libvips 8.18.7, libheif >= 1.22.0 and ffmpeg (libx264, native `aac`, libzimg) in the decoder image.
- Production code that must exist: the real build stages producing those libraries in the shipped image.
- Unacceptable substitutes: Debian's own libvips or ffmpeg packages where the docs require source builds, or an image without media tools used for any test that claims a media outcome.

## Constraints

- Builder and runtime share one pinned Debian release so libraries link cleanly.
- The exact demuxer and decoder lists come from gate S5 (E-19/c01); the loader allowlist addition from S4 (E-19/c02).

## Risks & Mitigation

**Risk 1: A trimmed list loses a phone format**
- *Risk*: a phone demuxer is missing, so real clips fail.
- *Mitigation*: lists come from the S5 run over the phone corpus.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The E-19 epic and 10 §1 list the image build recipe as out of scope ("deployment docs"), while E-00/c11 assigns the media build stages, pin list, configure recipe, SBOM and source offer to E-19. Without this task the decoder has no media tools (Runtime Completeness). Added as a split of E-19/c14; owner to confirm | E-19 owner, parent | open | Medium |
| The weekly rebuild and release workflows (push, sign, scan, source asset upload) have no owner in E-19; E-00/c11 says "release workflows, E-19 and later" | Release / CI owner | open | Medium |
